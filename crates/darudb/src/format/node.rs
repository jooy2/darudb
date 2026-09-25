//! The content of leaf and branch pages, and the reference a leaf keeps to a
//! value stored in an overflow run.
//!
//! A leaf's content starts with a slot array: one 2-byte offset per entry, in
//! key order. The entries sit at the end of the page, before the check:
//!
//! | Size | Field                                             |
//! | ---- | ------------------------------------------------- |
//! | 2    | Key length `K`                                    |
//! | 1    | Value kind: 0 inline, 1 overflow                  |
//! | 2    | Inline only: value length `V`                     |
//! | `K`  | Key                                               |
//! | `V`  | Inline only: the value                            |
//! | 44   | Overflow only: the overflow reference             |
//!
//! A branch with `k` keys holds `k + 1` child pointers, then a slot array of
//! `k` offsets, then the keys at the end of the page, each a 2-byte length and
//! the key. Child 0 holds the keys below key 0; child `i` holds the keys from
//! key `i − 1` up to key `i`; the last child holds the rest.
//!
//! Decoding validates every offset and length against the page, so a damaged
//! or crafted page produces an error, never a read outside it.

use std::cmp::Ordering;

use super::check::Check;
use super::page::{CONTENT_OFFSET, check_offset, content_len};
use super::pointer::{POINTER_LEN, Pointer};
use super::{le_u32, le_u64};

/// The size of an overflow reference, in bytes.
pub(crate) const OVERFLOW_REF_LEN: usize = 44;

/// The most bytes a leaf entry spends besides its key: slot, key length,
/// value kind and an overflow reference.
const LEAF_OVERHEAD: usize = 2 + 2 + 1 + OVERFLOW_REF_LEN;

/// The longest key a page of `page_size` bytes allows: four entries of the
/// largest kind always fit in one node, so a node that overflows can always be
/// split into two valid ones.
pub(crate) fn max_key_len(page_size: usize) -> usize {
    (content_len(page_size) - 4 * LEAF_OVERHEAD) / 4
}

/// The largest leaf entry, slot included, that keeps its value inline. A
/// value that would make its entry larger goes into an overflow run.
pub(crate) fn inline_limit(page_size: usize) -> usize {
    content_len(page_size) / 4
}

/// The number of pages an overflow run of `len` bytes needs.
pub(crate) fn overflow_pages(page_size: usize, len: u64) -> u64 {
    len.div_ceil(content_len(page_size) as u64).max(1)
}

/// Where a value too large to keep inline is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverflowRef {
    /// The first page of the run.
    pub(crate) first: u64,
    /// The commit that wrote the run.
    pub(crate) txn: u64,
    /// The number of pages in the run.
    pub(crate) pages: u32,
    /// The length of the value, in bytes.
    pub(crate) len: u64,
    /// XXH3-128 of the checks of the run's pages, concatenated in order.
    pub(crate) check: Check,
}

impl OverflowRef {
    fn write(&self, bytes: &mut [u8]) {
        bytes[0..8].copy_from_slice(&self.first.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.txn.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.pages.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.len.to_le_bytes());
        self.check.write(&mut bytes[28..]);
    }

    fn read(bytes: &[u8], page_size: usize) -> Result<Self, &'static str> {
        let reference = Self {
            first: le_u64(bytes, 0),
            txn: le_u64(bytes, 8),
            pages: le_u32(bytes, 16),
            len: le_u64(bytes, 20),
            check: Check::read(&bytes[28..]),
        };

        if reference.first == 0 || reference.txn == 0 {
            return Err("an overflow reference points at no page");
        }

        if u64::from(reference.pages) != overflow_pages(page_size, reference.len) {
            return Err("an overflow reference's page count does not fit its length");
        }

        Ok(reference)
    }
}

/// A value as a leaf stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoredValue {
    /// Kept in the leaf.
    Inline(Vec<u8>),
    /// Kept in an overflow run.
    Overflow(OverflowRef),
}

/// A value as a leaf stores it, read in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredRef<'a> {
    Inline(&'a [u8]),
    Overflow(OverflowRef),
}

impl StoredValue {
    /// The value, borrowed.
    pub(crate) fn as_stored(&self) -> StoredRef<'_> {
        match self {
            StoredValue::Inline(value) => StoredRef::Inline(value),
            StoredValue::Overflow(reference) => StoredRef::Overflow(*reference),
        }
    }
}

/// One entry of a leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeafEntry {
    /// The key.
    pub(crate) key: Vec<u8>,
    /// The value, or where it is.
    pub(crate) value: StoredValue,
}

impl LeafEntry {
    /// The bytes this entry takes in a leaf, slot included.
    pub(crate) fn len(&self) -> usize {
        match &self.value {
            StoredValue::Inline(value) => inline_entry_len(self.key.len(), value.len()),
            StoredValue::Overflow(_) => LEAF_OVERHEAD + self.key.len(),
        }
    }
}

/// The bytes an entry with an inline value takes in a leaf, slot included.
pub(crate) fn inline_entry_len(key_len: usize, value_len: usize) -> usize {
    2 + 2 + 1 + 2 + key_len + value_len
}

/// The bytes a branch with these keys takes, including one more child than
/// keys.
pub(crate) fn branch_len<K: AsRef<[u8]>>(keys: &[K]) -> usize {
    POINTER_LEN
        + keys
            .iter()
            .map(|key| branch_key_len(key.as_ref().len()))
            .sum::<usize>()
}

/// The bytes one key and the child after it take in a branch.
pub(crate) fn branch_key_len(key_len: usize) -> usize {
    POINTER_LEN + 2 + 2 + key_len
}

/// An offset inside a page, which is at most 65536 bytes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "every offset lies inside a page of at most 65536 bytes"
)]
fn offset(value: usize) -> [u8; 2] {
    debug_assert!(value <= usize::from(u16::MAX));

    (value as u16).to_le_bytes()
}

fn read_u16(page: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([page[at], page[at + 1]]))
}

/// The bytes an entry's cell takes in a leaf, its slot not included.
pub(crate) fn cell_len(key_len: usize, value: StoredRef<'_>) -> usize {
    match value {
        StoredRef::Inline(value) => inline_entry_len(key_len, value.len()) - 2,
        StoredRef::Overflow(_) => LEAF_OVERHEAD - 2 + key_len,
    }
}

/// Writes an entry's cell, of [`cell_len`] bytes, at offset `at` of a leaf.
pub(crate) fn write_cell(page: &mut [u8], at: usize, key: &[u8], value: StoredRef<'_>) {
    page[at..at + 2].copy_from_slice(&offset(key.len()));

    match value {
        StoredRef::Inline(value) => {
            page[at + 2] = 0;
            page[at + 3..at + 5].copy_from_slice(&offset(value.len()));
            page[at + 5..at + 5 + key.len()].copy_from_slice(key);
            page[at + 5 + key.len()..at + 5 + key.len() + value.len()].copy_from_slice(value);
        }
        StoredRef::Overflow(reference) => {
            page[at + 2] = 1;
            page[at + 3..at + 3 + key.len()].copy_from_slice(key);
            reference.write(&mut page[at + 3 + key.len()..]);
        }
    }
}

/// Where the cell of entry `index` of a leaf that [`check_leaf`] passed
/// lies, and how long it is.
pub(crate) fn leaf_cell(page: &[u8], index: usize) -> (usize, usize) {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);
    let key_len = read_u16(page, at);
    let len = if page[at + 2] == 0 {
        inline_entry_len(key_len, read_u16(page, at + 3)) - 2
    } else {
        LEAF_OVERHEAD - 2 + key_len
    };

    (at, len)
}

/// Points slot `index` of a leaf at offset `at`.
pub(crate) fn set_slot(page: &mut [u8], index: usize, at: usize) {
    let slot = CONTENT_OFFSET + 2 * index;

    page[slot..slot + 2].copy_from_slice(&offset(at));
}

/// Writes `entries` as the content of a leaf. The caller has checked that
/// they fit.
pub(crate) fn encode_leaf(entries: &[LeafEntry], page: &mut [u8]) {
    let mut cursor = check_offset(page.len());

    for (index, entry) in entries.iter().enumerate() {
        let value = entry.value.as_stored();

        cursor -= cell_len(entry.key.len(), value);
        write_cell(page, cursor, &entry.key, value);
        set_slot(page, index, cursor);
    }

    debug_assert!(cursor >= CONTENT_OFFSET + 2 * entries.len());
}

/// Checks every offset, length and value kind of a leaf with `count`
/// entries against the page, and the order of its keys, so that
/// [`leaf_key`] and [`leaf_value`] can read it in place.
pub(crate) fn check_leaf(page: &[u8], count: usize) -> Result<(), &'static str> {
    let page_size = page.len();
    let end = check_offset(page_size);
    let slots_end = CONTENT_OFFSET + 2 * count;
    let max_key = max_key_len(page_size);

    if count == 0 {
        return Err("a leaf holds no entry");
    }

    if slots_end > end {
        return Err("a leaf's slots run past its content");
    }

    let mut previous: Option<&[u8]> = None;

    for index in 0..count {
        let at = read_u16(page, CONTENT_OFFSET + 2 * index);

        if at < slots_end || at + 3 > end {
            return Err("a leaf entry lies outside the page");
        }

        let key_len = read_u16(page, at);

        if key_len > max_key {
            return Err("a leaf key is longer than the page allows");
        }

        match page[at + 2] {
            0 => {
                if at + 5 > end || at + 5 + key_len + read_u16(page, at + 3) > end {
                    return Err("a leaf entry lies outside the page");
                }
            }
            1 => {
                if at + 3 + key_len + OVERFLOW_REF_LEN > end {
                    return Err("a leaf entry lies outside the page");
                }

                OverflowRef::read(&page[at + 3 + key_len..], page_size)?;
            }
            _ => return Err("a leaf entry's value is of no known kind"),
        }

        let key = leaf_key(page, index);

        if previous.is_some_and(|previous| previous.cmp(key) != Ordering::Less) {
            return Err("a leaf's keys are out of order");
        }

        previous = Some(key);
    }

    Ok(())
}

/// The key of entry `index` of a leaf that [`check_leaf`] passed.
pub(crate) fn leaf_key(page: &[u8], index: usize) -> &[u8] {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);
    let key_len = read_u16(page, at);
    let start = if page[at + 2] == 0 { at + 5 } else { at + 3 };

    &page[start..start + key_len]
}

/// The value of entry `index` of a leaf that [`check_leaf`] passed.
pub(crate) fn leaf_value(page: &[u8], index: usize) -> Result<StoredRef<'_>, &'static str> {
    leaf_entry(page, index).map(|(_, value)| value)
}

/// The key and the value of entry `index` of a leaf that [`check_leaf`]
/// passed, if the value is inline: nearly every value, in a return small
/// enough to pass in registers.
pub(crate) fn leaf_inline(page: &[u8], index: usize) -> Option<(&[u8], &[u8])> {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);

    if page[at + 2] != 0 {
        return None;
    }

    let key_end = at + 5 + read_u16(page, at);

    Some((
        &page[at + 5..key_end],
        &page[key_end..key_end + read_u16(page, at + 3)],
    ))
}

/// The key and the value of entry `index` of a leaf that [`check_leaf`]
/// passed, read together.
pub(crate) fn leaf_entry(
    page: &[u8],
    index: usize,
) -> Result<(&[u8], StoredRef<'_>), &'static str> {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);
    let key_len = read_u16(page, at);

    Ok(if page[at + 2] == 0 {
        let start = at + 5 + key_len;

        (
            &page[at + 5..start],
            StoredRef::Inline(&page[start..start + read_u16(page, at + 3)]),
        )
    } else {
        (
            &page[at + 3..at + 3 + key_len],
            StoredRef::Overflow(OverflowRef::read(&page[at + 3 + key_len..], page.len())?),
        )
    })
}

/// The bytes the `count` entries of a leaf that [`check_leaf`] passed take,
/// slots included, as [`LeafEntry::len`] counts them, and where its lowest
/// cell starts: the end of its free space, `check_offset` of the page when
/// it has no entry.
pub(crate) fn leaf_extent(page: &[u8], count: usize) -> (usize, usize) {
    (0..count).fold((0, check_offset(page.len())), |(size, low), index| {
        let at = read_u16(page, CONTENT_OFFSET + 2 * index);
        let key_len = read_u16(page, at);
        let len = if page[at + 2] == 0 {
            inline_entry_len(key_len, read_u16(page, at + 3))
        } else {
            LEAF_OVERHEAD + key_len
        };

        (size + len, low.min(at))
    })
}

/// Reads the `count` entries of a leaf.
#[cfg(test)]
pub(crate) fn decode_leaf(page: &[u8], count: usize) -> Result<Vec<LeafEntry>, &'static str> {
    check_leaf(page, count)?;

    (0..count)
        .map(|index| {
            Ok(LeafEntry {
                key: leaf_key(page, index).to_vec(),
                value: match leaf_value(page, index)? {
                    StoredRef::Inline(value) => StoredValue::Inline(value.to_vec()),
                    StoredRef::Overflow(reference) => StoredValue::Overflow(reference),
                },
            })
        })
        .collect()
}

/// Writes a branch's keys and children as its content. There is one more
/// child than keys, and the caller has checked that they fit.
pub(crate) fn encode_branch<K: AsRef<[u8]>>(keys: &[K], children: &[Pointer], page: &mut [u8]) {
    debug_assert_eq!(children.len(), keys.len() + 1);

    for (index, child) in children.iter().enumerate() {
        child.write(&mut page[CONTENT_OFFSET + POINTER_LEN * index..]);
    }

    let slots = CONTENT_OFFSET + POINTER_LEN * children.len();
    let mut cursor = check_offset(page.len());

    for (index, key) in keys.iter().enumerate() {
        let key = key.as_ref();

        cursor -= 2 + key.len();
        page[cursor..cursor + 2].copy_from_slice(&offset(key.len()));
        page[cursor + 2..cursor + 2 + key.len()].copy_from_slice(key);

        let slot = slots + 2 * index;

        page[slot..slot + 2].copy_from_slice(&offset(cursor));
    }

    debug_assert!(cursor >= slots + 2 * keys.len());
}

/// Checks the children, the key offsets and lengths of a branch with
/// `count` keys against the page, and the order of its keys, so that
/// [`branch_key`] and [`branch_child`] can read it in place.
pub(crate) fn check_branch(page: &[u8], count: usize) -> Result<(), &'static str> {
    let end = check_offset(page.len());
    let slots = CONTENT_OFFSET + POINTER_LEN * (count + 1);
    let slots_end = slots + 2 * count;
    let max_key = max_key_len(page.len());

    if count == 0 {
        return Err("a branch holds no key");
    }

    if slots_end > end {
        return Err("a branch's children run past its content");
    }

    for index in 0..=count {
        let child = branch_child(page, index);

        if child.page == 0 || child.txn == 0 {
            return Err("a branch points at no page");
        }
    }

    let mut previous: Option<&[u8]> = None;

    for index in 0..count {
        let at = read_u16(page, slots + 2 * index);

        if at < slots_end || at + 2 > end {
            return Err("a branch key lies outside the page");
        }

        let key_len = read_u16(page, at);

        if key_len > max_key || at + 2 + key_len > end {
            return Err("a branch key lies outside the page");
        }

        let key = &page[at + 2..at + 2 + key_len];

        if previous.is_some_and(|previous| previous >= key) {
            return Err("a branch's keys are out of order");
        }

        previous = Some(key);
    }

    Ok(())
}

/// Key `index` of a branch with `count` keys that [`check_branch`] passed.
pub(crate) fn branch_key(page: &[u8], count: usize, index: usize) -> &[u8] {
    let at = read_u16(page, CONTENT_OFFSET + POINTER_LEN * (count + 1) + 2 * index);

    &page[at + 2..at + 2 + read_u16(page, at)]
}

/// Child `index` of a branch.
pub(crate) fn branch_child(page: &[u8], index: usize) -> Pointer {
    Pointer::read(&page[CONTENT_OFFSET + POINTER_LEN * index..])
}

/// The bytes a branch with `count` keys that [`check_branch`] passed takes,
/// as [`branch_len`] counts them.
pub(crate) fn branch_size(page: &[u8], count: usize) -> usize {
    POINTER_LEN
        + (0..count)
            .map(|index| branch_key_len(branch_key(page, count, index).len()))
            .sum::<usize>()
}

/// Reads the `count` keys and `count + 1` children of a branch.
pub(crate) fn decode_branch(
    page: &[u8],
    count: usize,
) -> Result<(Vec<Vec<u8>>, Vec<Pointer>), &'static str> {
    check_branch(page, count)?;

    Ok((
        (0..count)
            .map(|index| branch_key(page, count, index).to_vec())
            .collect(),
        (0..=count).map(|index| branch_child(page, index)).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: usize = 4096;

    fn inline(key: &[u8], value: &[u8]) -> LeafEntry {
        LeafEntry {
            key: key.to_vec(),
            value: StoredValue::Inline(value.to_vec()),
        }
    }

    fn reference() -> OverflowRef {
        OverflowRef {
            first: 9,
            txn: 3,
            pages: 2,
            len: 5000,
            check: Check::of(&[b"run"]),
        }
    }

    #[test]
    fn the_limits_are_the_documented_ones() {
        assert_eq!(max_key_len(4096), 957);
        assert_eq!(max_key_len(16384), 4029);
        assert_eq!(max_key_len(65536), 16317);
        assert_eq!(inline_limit(4096), 1006);
        assert_eq!(overflow_pages(4096, 4024), 1);
        assert_eq!(overflow_pages(4096, 4025), 2);
    }

    #[test]
    fn four_of_the_largest_entries_always_fit() {
        for page_size in [4096, 16384, 65536] {
            let key = max_key_len(page_size);
            let leaf = 4 * (LEAF_OVERHEAD + key);
            let branch = branch_len(&vec![vec![0u8; key]; 4]);

            assert!(leaf <= content_len(page_size), "{page_size}");
            assert!(branch <= content_len(page_size), "{page_size}");
        }
    }

    #[test]
    fn a_leaf_reads_back_as_it_was_written() {
        let entries = vec![
            inline(b"", b"the empty key"),
            inline(b"apple", b""),
            LeafEntry {
                key: b"banana".to_vec(),
                value: StoredValue::Overflow(reference()),
            },
            inline(b"cherry", &[0xAB; 300]),
        ];
        let mut page = vec![0u8; P];

        encode_leaf(&entries, &mut page);

        assert_eq!(decode_leaf(&page, entries.len()), Ok(entries));
    }

    #[test]
    fn a_leaf_entry_has_the_documented_layout() {
        let mut page = vec![0u8; P];

        encode_leaf(&[inline(b"ab", b"xyz")], &mut page);

        let at = read_u16(&page, CONTENT_OFFSET);

        assert_eq!(at, check_offset(P) - 10, "packed against the end");
        assert_eq!(
            &page[at..at + 10],
            &[2, 0, 0, 3, 0, b'a', b'b', b'x', b'y', b'z']
        );
    }

    #[test]
    fn a_branch_reads_back_as_it_was_written() {
        let keys = vec![b"m".to_vec(), b"t".to_vec()];
        let children: Vec<Pointer> = (1..=3)
            .map(|page| Pointer {
                page,
                txn: 2,
                check: Check::of(&[&page.to_le_bytes()]),
            })
            .collect();
        let mut page = vec![0u8; P];

        encode_branch(&keys, &children, &mut page);

        assert_eq!(decode_branch(&page, 2), Ok((keys, children)));
        assert_eq!(
            Pointer::read(&page[CONTENT_OFFSET..]).page,
            1,
            "child 0 first"
        );
    }

    #[test]
    fn keys_out_of_order_are_refused() {
        let mut page = vec![0u8; P];

        encode_leaf(&[inline(b"b", b""), inline(b"a", b"")], &mut page);

        assert!(decode_leaf(&page, 2).is_err());

        encode_leaf(&[inline(b"a", b""), inline(b"a", b"")], &mut page);

        assert!(decode_leaf(&page, 2).is_err(), "a key twice");
    }

    #[test]
    fn offsets_outside_the_page_are_refused_rather_than_followed() {
        let mut page = vec![0u8; P];

        encode_leaf(&[inline(b"key", b"value")], &mut page);

        // A slot that points into the check, and a key length that runs out.
        page[CONTENT_OFFSET..CONTENT_OFFSET + 2].copy_from_slice(&offset(P - 8));

        assert!(decode_leaf(&page, 1).is_err());

        encode_leaf(&[inline(b"key", b"value")], &mut page);

        let at = read_u16(&page, CONTENT_OFFSET);

        page[at..at + 2].copy_from_slice(&offset(900));

        assert!(decode_leaf(&page, 1).is_err());
        assert!(
            decode_leaf(&page, 3000).is_err(),
            "more slots than the page holds"
        );
        assert!(decode_leaf(&page, 0).is_err(), "an empty leaf");
    }

    #[test]
    fn a_branch_pointing_at_page_zero_is_refused() {
        let mut page = vec![0u8; P];

        encode_branch(&[b"k"], &[Pointer::NULL, Pointer::NULL], &mut page);

        assert!(decode_branch(&page, 1).is_err());
    }

    #[test]
    fn an_overflow_reference_whose_length_and_pages_disagree_is_refused() {
        let mut bad = reference();

        bad.pages = 3;

        let mut page = vec![0u8; P];

        encode_leaf(
            &[LeafEntry {
                key: b"k".to_vec(),
                value: StoredValue::Overflow(bad),
            }],
            &mut page,
        );

        assert!(decode_leaf(&page, 1).is_err());
    }
}
