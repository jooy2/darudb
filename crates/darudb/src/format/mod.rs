//! What the bytes of a database file mean.
//!
//! Everything in this module is a pure function over bytes: nothing here opens
//! a file or knows a path. That keeps every rule about the on-disk layout
//! testable without a disk, and keeps the layout in one place, where a change
//! to it is easy to see in review. `design/file-format.md` is the
//! specification this module implements, and its tests pin the offsets the
//! specification documents.
//!
//! Any change to what is written to disk changes [`FORMAT_VERSION`]. Until the
//! first release there are no migrations between versions: a file in another
//! version is refused when it is opened.
//!
//! `object` holds the object layer's encodings of `design/objects.md`, kept in
//! the kernel's trees. They change nothing the kernel reads, and the stored
//! schema carries a version of its own for them.

mod check;
mod header;
mod key_block;
mod node;
pub(crate) mod object;
mod page;
mod pointer;
mod record;
mod system;

pub(crate) use check::Check;
pub(crate) use header::{
    Cipher, HEADER_LEN, HeaderError, SELECTOR_OFFSET, SLOT_COUNT, STATIC_LEN, Selector,
    StaticHeader, slot_offset,
};
pub(crate) use key_block::{Kdf, KeyBlock};
pub(crate) use node::{
    LeafEntry, OverflowRef, StoredRef, StoredValue, branch_child, branch_key, branch_key_len,
    branch_len, branch_size, check_branch, check_leaf, decode_branch, decode_leaf, encode_branch,
    encode_leaf, inline_entry_len, inline_limit, leaf_entry, leaf_inline, leaf_key, leaf_size,
    leaf_value, max_key_len, overflow_pages,
};
pub(crate) use page::{
    CONTENT_OFFSET, PAGE_HEADER_OFFSET, PageHeader, PageKind, check_offset, content_len,
    page_check, seal, stored_check,
};
pub(crate) use pointer::{POINTER_LEN, Pointer};
pub(crate) use record::{CommitRecord, KEY_BLOCK_LEN, RECORD_LEN, RECORD_MAC_LEN, TXN_LIMIT};
pub(crate) use system::{
    CATALOG_TREE, FREE_TREE, RETAINED_TREE, TreeDescriptor, decode_free_key, decode_free_value,
    decode_retained_key, decode_runs, encode_runs, free_key, free_value, retained_key,
    runs_per_value,
};

/// The file format version this build of the library reads and writes.
///
/// It is recorded in every file's header. A file with another version is
/// refused with [`Error::UnsupportedFormatVersion`](crate::Error::UnsupportedFormatVersion).
pub const FORMAT_VERSION: u32 = 3;

/// The first eight bytes of every DaruDB file.
///
/// The leading `0x89` is outside ASCII, so the file is never mistaken for text
/// and a transfer that strips the eighth bit is detected. The trailing `\n`
/// catches a transfer that rewrites line endings.
pub(crate) const MAGIC: [u8; 8] = *b"\x89DaruDB\n";

/// The page size of a new database when the caller does not choose one.
///
/// A placeholder rather than a measured choice; the benchmarks decide the real
/// default. It is independent of the operating system's memory page size,
/// which the engine never assumes, since it reads and writes the file without
/// mapping it into memory.
pub(crate) const DEFAULT_PAGE_SIZE: u32 = 4096;

/// The smallest page size a database may use. Page 0 needs 2048 bytes, and
/// smaller pages would save nothing on storage whose sectors are 4096 bytes.
pub(crate) const MIN_PAGE_SIZE: u32 = 4096;

/// The largest page size a database may use.
pub(crate) const MAX_PAGE_SIZE: u32 = 65536;

/// The id of the first tree created by the layers above the storage kernel.
/// Ids below it belong to the engine.
pub(crate) const FIRST_USER_TREE: u64 = 16;

/// Whether `size` can be a database's page size: a power of two between
/// [`MIN_PAGE_SIZE`] and [`MAX_PAGE_SIZE`], inclusive.
pub(crate) fn is_valid_page_size(size: u32) -> bool {
    size.is_power_of_two() && (MIN_PAGE_SIZE..=MAX_PAGE_SIZE).contains(&size)
}

/// The little-endian `u32` at `offset`. The caller has checked the length.
pub(crate) fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut field = [0u8; 4];

    field.copy_from_slice(&bytes[offset..offset + 4]);

    u32::from_le_bytes(field)
}

/// The little-endian `u64` at `offset`. The caller has checked the length.
pub(crate) fn le_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut field = [0u8; 8];

    field.copy_from_slice(&bytes[offset..offset + 8]);

    u64::from_le_bytes(field)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_sizes_are_powers_of_two_within_the_bounds() {
        assert!(is_valid_page_size(MIN_PAGE_SIZE));
        assert!(is_valid_page_size(DEFAULT_PAGE_SIZE));
        assert!(is_valid_page_size(16384));
        assert!(is_valid_page_size(MAX_PAGE_SIZE));

        assert!(!is_valid_page_size(0));
        assert!(!is_valid_page_size(512));
        assert!(!is_valid_page_size(MIN_PAGE_SIZE / 2));
        assert!(!is_valid_page_size(MAX_PAGE_SIZE * 2));
        assert!(!is_valid_page_size(4097));
        assert!(!is_valid_page_size(12288));
    }

    #[test]
    fn the_default_page_size_is_a_valid_one() {
        assert!(is_valid_page_size(DEFAULT_PAGE_SIZE));
    }
}
