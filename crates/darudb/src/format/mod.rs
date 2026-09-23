//! What the bytes of a database file mean.
//!
//! Everything in this module is a pure function over bytes: nothing here opens
//! a file or knows a path. That keeps every rule about the on-disk layout
//! testable without a disk, and keeps the layout in one place, where a change
//! to it is easy to see in review.
//!
//! Any change to what is written to disk changes [`FORMAT_VERSION`]. Until the
//! first release there are no migrations between versions: a file in another
//! version is refused when it is opened.

mod header;

pub(crate) use header::{FileHeader, HEADER_LEN, HeaderError};

/// The file format version this build of the library reads and writes.
///
/// It is recorded in every file's header. A file with another version is
/// refused with [`Error::UnsupportedFormatVersion`](crate::Error::UnsupportedFormatVersion).
pub const FORMAT_VERSION: u32 = 1;

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

/// The smallest page size a database may use.
pub(crate) const MIN_PAGE_SIZE: u32 = 512;

/// The largest page size a database may use.
pub(crate) const MAX_PAGE_SIZE: u32 = 65536;

/// Whether `size` can be a database's page size: a power of two between
/// [`MIN_PAGE_SIZE`] and [`MAX_PAGE_SIZE`], inclusive.
pub(crate) fn is_valid_page_size(size: u32) -> bool {
    size.is_power_of_two() && (MIN_PAGE_SIZE..=MAX_PAGE_SIZE).contains(&size)
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
