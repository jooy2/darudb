//! The frame every page other than page 0 shares.
//!
//! | Offset   | Size | Content                                            |
//! | -------- | ---- | -------------------------------------------------- |
//! | 0        | 24   | Prefix: the nonce in an encrypted file, else zeros |
//! | 24       | 32   | Page header                                        |
//! | 56       | `C`  | Content                                            |
//! | `P − 16` | 16   | The page's check                                   |
//!
//! `C`, the content area, is `P − 72` bytes whether or not the file is
//! encrypted, so a tree has the same shape either way.

use super::check::{CHECK_LEN, Check};
use super::le_u64;

/// Where the page header starts, after the prefix that holds an encrypted
/// page's nonce.
pub(crate) const PAGE_HEADER_OFFSET: usize = 24;

/// Where the content area starts.
pub(crate) const CONTENT_OFFSET: usize = 56;

/// The bytes of a page that are not content: prefix, header and check.
const ENVELOPE_LEN: usize = CONTENT_OFFSET + CHECK_LEN;

/// The size of the content area `C` of a page of `page_size` bytes.
pub(crate) fn content_len(page_size: usize) -> usize {
    page_size - ENVELOPE_LEN
}

/// Where the check of a page of `page_size` bytes starts; the content ends
/// there.
pub(crate) fn check_offset(page_size: usize) -> usize {
    page_size - CHECK_LEN
}

/// What a page holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageKind {
    /// A leaf of a B+tree.
    Leaf = 1,
    /// A branch of a B+tree.
    Branch = 2,
    /// One page of an overflow run.
    Overflow = 3,
}

/// The 32-byte header at offset 24 of every page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageHeader {
    /// What the page holds.
    pub(crate) kind: PageKind,
    /// 0 for a leaf or an overflow page; one more than its children for a
    /// branch.
    pub(crate) level: u8,
    /// Entries in a leaf, keys in a branch, 0 in an overflow page.
    pub(crate) count: u16,
    /// The commit that wrote the page.
    pub(crate) txn: u64,
    /// The tree the page belongs to.
    pub(crate) tree: u64,
    /// For an overflow page, its position in its run; otherwise 0.
    pub(crate) index: u64,
}

impl PageHeader {
    /// Writes the header into its place in `page`.
    pub(crate) fn write(&self, page: &mut [u8]) {
        let header = &mut page[PAGE_HEADER_OFFSET..CONTENT_OFFSET];

        header[0] = self.kind as u8;
        header[1] = self.level;
        header[2..4].copy_from_slice(&self.count.to_le_bytes());
        header[4..8].copy_from_slice(&[0; 4]);
        header[8..16].copy_from_slice(&self.txn.to_le_bytes());
        header[16..24].copy_from_slice(&self.tree.to_le_bytes());
        header[24..32].copy_from_slice(&self.index.to_le_bytes());
    }

    /// Reads the header of `page`.
    pub(crate) fn read(page: &[u8]) -> Result<Self, &'static str> {
        let header = &page[PAGE_HEADER_OFFSET..CONTENT_OFFSET];
        let kind = match header[0] {
            1 => PageKind::Leaf,
            2 => PageKind::Branch,
            3 => PageKind::Overflow,
            _ => return Err("the page is of no known kind"),
        };
        let level = header[1];

        if (kind == PageKind::Branch) != (level > 0) {
            return Err("the page's level does not match its kind");
        }

        Ok(Self {
            kind,
            level,
            count: u16::from_le_bytes([header[2], header[3]]),
            txn: le_u64(header, 8),
            tree: le_u64(header, 16),
            index: le_u64(header, 24),
        })
    }
}

/// The check of plain page `page_number`: XXH3-128 of the page number, then
/// every byte of the page before the check.
pub(crate) fn page_check(page_number: u64, page: &[u8]) -> Check {
    let end = check_offset(page.len());

    Check::of(&[&page_number.to_le_bytes(), &page[..end]])
}

/// Computes the check of plain page `page_number`, stores it at the end of the
/// page, and returns it.
pub(crate) fn seal(page_number: u64, page: &mut [u8]) -> Check {
    let check = page_check(page_number, page);
    let end = check_offset(page.len());

    check.write(&mut page[end..]);

    check
}

/// The check stored at the end of `page`.
pub(crate) fn stored_check(page: &[u8]) -> Check {
    Check::read(&page[check_offset(page.len())..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_is_the_documented_one() {
        assert_eq!(content_len(4096), 4024);
        assert_eq!(content_len(65536), 65464);
        assert_eq!(check_offset(4096), 4080);
    }

    #[test]
    fn a_header_reads_back_as_it_was_written() {
        let header = PageHeader {
            kind: PageKind::Branch,
            level: 2,
            count: 300,
            txn: 7,
            tree: 16,
            index: 0,
        };
        let mut page = vec![0u8; 4096];

        header.write(&mut page);

        assert_eq!(PageHeader::read(&page), Ok(header));
        assert_eq!(page[24], 2, "kind");
        assert_eq!(page[25], 2, "level");
        assert_eq!(&page[26..28], &300u16.to_le_bytes());
        assert_eq!(&page[32..40], &7u64.to_le_bytes());
        assert_eq!(&page[40..48], &16u64.to_le_bytes());
    }

    #[test]
    fn a_kind_and_a_level_that_disagree_are_refused() {
        let mut page = vec![0u8; 4096];

        PageHeader {
            kind: PageKind::Leaf,
            level: 0,
            count: 1,
            txn: 1,
            tree: 16,
            index: 0,
        }
        .write(&mut page);
        page[25] = 1;

        assert!(PageHeader::read(&page).is_err());

        page[24] = 9;

        assert!(PageHeader::read(&page).is_err());
    }

    #[test]
    fn a_sealed_page_carries_its_check_which_covers_its_number() {
        let mut page = vec![7u8; 4096];
        let check = seal(12, &mut page);

        assert_eq!(stored_check(&page), check);
        assert_eq!(page_check(12, &page), check);
        assert_ne!(
            page_check(13, &page),
            check,
            "the same bytes at another place"
        );
    }
}
