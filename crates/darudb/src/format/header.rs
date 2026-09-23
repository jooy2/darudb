//! The file header, which opens page 0 of every database file.
//!
//! The header says what the file is and how to read the rest of it. Today it
//! holds three fields, all little-endian:
//!
//! | Offset | Size | Field                                              |
//! | ------ | ---- | -------------------------------------------------- |
//! | 0      | 8    | [`MAGIC`](super::MAGIC)                            |
//! | 8      | 4    | The file format version                            |
//! | 12     | 4    | The page size, in bytes                            |
//!
//! The rest of page 0 is reserved and written as zeros. The two commit slots,
//! the flag byte that picks between them, and the header's own checksum come
//! with the storage kernel, together with the format version that describes
//! them.

use super::{FORMAT_VERSION, MAGIC, is_valid_page_size};

/// The number of bytes of page 0 the header occupies.
pub(crate) const HEADER_LEN: usize = 16;

/// The fields of a file header, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileHeader {
    /// The file format version the file was written in.
    pub(crate) format_version: u32,
    /// The size of every page in the file, in bytes.
    pub(crate) page_size: u32,
}

/// Why a run of bytes is not a header this build can use.
///
/// This module knows nothing about paths, so the caller turns each of these
/// into an [`Error`](crate::Error) that names the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderError {
    /// Too short to be a header, or not starting with [`MAGIC`](super::MAGIC).
    NotADatabase,
    /// A DaruDB header in another format version, whose layout past the
    /// version field this build cannot know.
    UnsupportedVersion(u32),
    /// A page size no DaruDB file can have, so the header has been damaged.
    InvalidPageSize(u32),
}

impl FileHeader {
    /// The header of a new file in this build's format version.
    pub(crate) fn new(page_size: u32) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            page_size,
        }
    }

    /// The header as the bytes written at offset 0 of the file.
    pub(crate) fn encode(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0u8; HEADER_LEN];

        bytes[0..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&self.format_version.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.page_size.to_le_bytes());

        bytes
    }

    /// Reads a header from the bytes at offset 0 of a file.
    ///
    /// The version is checked before anything after it, because a header in
    /// another version may lay out the rest differently.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, HeaderError> {
        let Some(bytes) = bytes.get(..HEADER_LEN) else {
            return Err(HeaderError::NotADatabase);
        };

        if bytes[0..8] != MAGIC {
            return Err(HeaderError::NotADatabase);
        }

        let format_version = read_u32(bytes, 8);

        if format_version != FORMAT_VERSION {
            return Err(HeaderError::UnsupportedVersion(format_version));
        }

        let page_size = read_u32(bytes, 12);

        if !is_valid_page_size(page_size) {
            return Err(HeaderError::InvalidPageSize(page_size));
        }

        Ok(Self {
            format_version,
            page_size,
        })
    }
}

/// The little-endian `u32` at `offset`. The caller has checked the length.
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut field = [0u8; 4];

    field.copy_from_slice(&bytes[offset..offset + 4]);

    u32::from_le_bytes(field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, MIN_PAGE_SIZE};

    #[test]
    fn a_header_reads_back_as_it_was_written() {
        for page_size in [MIN_PAGE_SIZE, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE] {
            let header = FileHeader::new(page_size);

            assert_eq!(FileHeader::decode(&header.encode()), Ok(header));
        }
    }

    #[test]
    fn the_layout_is_the_documented_one() {
        let bytes = FileHeader::new(4096).encode();

        assert_eq!(&bytes[0..8], b"\x89DaruDB\n");
        assert_eq!(&bytes[8..12], &FORMAT_VERSION.to_le_bytes());
        assert_eq!(&bytes[12..16], &[0x00, 0x10, 0x00, 0x00]);
    }

    #[test]
    fn bytes_after_the_header_are_ignored() {
        let mut page = vec![0xAB; 4096];

        page[..HEADER_LEN].copy_from_slice(&FileHeader::new(4096).encode());

        assert_eq!(FileHeader::decode(&page), Ok(FileHeader::new(4096)));
    }

    #[test]
    fn too_few_bytes_are_not_a_database() {
        let bytes = FileHeader::new(4096).encode();

        assert_eq!(FileHeader::decode(&[]), Err(HeaderError::NotADatabase));
        assert_eq!(
            FileHeader::decode(&bytes[..HEADER_LEN - 1]),
            Err(HeaderError::NotADatabase)
        );
    }

    #[test]
    fn other_leading_bytes_are_not_a_database() {
        let mut bytes = FileHeader::new(4096).encode();

        bytes[0] = b'D';

        assert_eq!(FileHeader::decode(&bytes), Err(HeaderError::NotADatabase));
    }

    #[test]
    fn another_format_version_is_reported_with_its_number() {
        let mut bytes = FileHeader::new(4096).encode();

        bytes[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());

        assert_eq!(
            FileHeader::decode(&bytes),
            Err(HeaderError::UnsupportedVersion(FORMAT_VERSION + 1))
        );
    }

    #[test]
    fn the_version_is_checked_before_the_page_size() {
        // A header in another version may keep something else where this one
        // keeps the page size, so that field must not be judged first.
        let mut bytes = FileHeader::new(4096).encode();

        bytes[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
        bytes[12..16].copy_from_slice(&3u32.to_le_bytes());

        assert_eq!(
            FileHeader::decode(&bytes),
            Err(HeaderError::UnsupportedVersion(FORMAT_VERSION + 1))
        );
    }

    #[test]
    fn an_impossible_page_size_is_reported() {
        for page_size in [0, 3, 4095, MIN_PAGE_SIZE / 2, MAX_PAGE_SIZE * 2] {
            let mut bytes = FileHeader::new(4096).encode();

            bytes[12..16].copy_from_slice(&page_size.to_le_bytes());

            assert_eq!(
                FileHeader::decode(&bytes),
                Err(HeaderError::InvalidPageSize(page_size))
            );
        }
    }
}
