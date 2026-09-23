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
