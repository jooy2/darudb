//! Page 0: the static fields, the selector and the three commit slots.
//!
//! | Offset | Size | Content                                  |
//! | ------ | ---- | ---------------------------------------- |
//! | 0      | 64   | Static fields, written once              |
//! | 64     | 1    | Selector                                 |
//! | 512    | 512  | Slot 0                                   |
//! | 1024   | 512  | Slot 1                                   |
//! | 1536   | 512  | Slot 2                                   |
//!
//! The rest of page 0 is reserved, but for the 64 bytes at 128, where raising
//! the format version writes the new static fields first ([`RAISED_OFFSET`]).
//! Each part sits in a 512-byte sector of its own, so that on a disk with
//! 512-byte sectors a write to one slot cannot damage another.
//! `design/file-format.md` is the specification.

use super::check::{CHECK_LEN, Check};
use super::{FORMAT_VERSION, MAGIC, OLDEST_FORMAT_VERSION, is_valid_page_size, le_u32};

/// The size of the static fields, in bytes.
pub(crate) const STATIC_LEN: usize = 64;

/// Where the selector byte is.
pub(crate) const SELECTOR_OFFSET: usize = 64;

/// The number of commit slots.
pub(crate) const SLOT_COUNT: usize = 3;

/// The size of one commit slot, in bytes.
pub(crate) const SLOT_LEN: usize = 512;

/// How many bytes at the start of page 0 mean anything: the static fields, the
/// selector and the slots.
pub(crate) const HEADER_LEN: usize = 2048;

/// Where the static check starts: it covers every byte before it.
pub(crate) const STATIC_CHECK_OFFSET: usize = STATIC_LEN - CHECK_LEN;

/// Where raising a file's format version writes its new static fields before
/// it changes the ones at the start of the file: in the first sector, beside
/// the static fields and the selector, at a place nothing else writes.
///
/// The static check covers the version, and only a one-byte write is
/// atomic, so the version and the check cannot change together. The new
/// version's one byte is written once this copy is durable, and the check
/// after it; a cut between the two leaves a version whose check fails,
/// which the copy, holding the same fields and a check that passes, tells
/// from damage.
pub(crate) const RAISED_OFFSET: usize = 128;

/// Where slot `slot` starts in the file.
pub(crate) fn slot_offset(slot: usize) -> usize {
    SLOT_LEN * (slot + 1)
}

/// How the pages of a file are protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cipher {
    /// Not encrypted: each page's check is a hash of it.
    Plain,
    /// Each page encrypted with XChaCha20-Poly1305: its check is its tag.
    XChaCha20Poly1305,
    /// Each page encrypted with XAES-256-GCM: its check is its tag.
    Xaes256Gcm,
}

/// The fields written when the file is created, of which only the format
/// version ever changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaticHeader {
    /// The file format version, from [`OLDEST_FORMAT_VERSION`] to
    /// [`FORMAT_VERSION`].
    pub(crate) version: u32,
    /// The size of every page, in bytes.
    pub(crate) page_size: u32,
    /// 16 random bytes that identify the file.
    pub(crate) file_id: [u8; 16],
    pub(crate) cipher: Cipher,
}

/// Why the bytes at the start of a file are not a header this build can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderError {
    /// Too short to hold the static fields, or not starting with the magic.
    NotADatabase,
    /// A DaruDB header in another format version.
    UnsupportedVersion(u32),
    /// A header of this version whose content is impossible.
    Damaged(&'static str),
}

impl StaticHeader {
    /// The static fields as the first 64 bytes of the file.
    pub(crate) fn encode(&self) -> [u8; STATIC_LEN] {
        let mut bytes = [0u8; STATIC_LEN];

        debug_assert!((OLDEST_FORMAT_VERSION..=FORMAT_VERSION).contains(&self.version));

        bytes[0..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&self.version.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.page_size.to_le_bytes());
        bytes[16..32].copy_from_slice(&self.file_id);
        bytes[32] = match self.cipher {
            Cipher::Plain => 0,
            Cipher::XChaCha20Poly1305 => 1,
            Cipher::Xaes256Gcm => 2,
        };

        let check = Check::of(&[&bytes[..STATIC_CHECK_OFFSET]]);

        check.write(&mut bytes[STATIC_CHECK_OFFSET..]);

        bytes
    }

    /// Reads the static fields from the start of a file, `bytes`, which
    /// holds the copy at [`RAISED_OFFSET`] too when it is long enough.
    ///
    /// The magic and the version are checked before the static check, because
    /// a file in another version may not have one where this version does. A
    /// version newer than the oldest whose check fails is read from the copy
    /// when the copy holds the same fields and passes its check: the process
    /// that raised the version stopped before it wrote the check.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, HeaderError> {
        let Some(fields) = bytes.get(..STATIC_LEN) else {
            return Err(HeaderError::NotADatabase);
        };

        if fields[0..8] != MAGIC {
            return Err(HeaderError::NotADatabase);
        }

        let version = le_u32(fields, 8);

        if !(OLDEST_FORMAT_VERSION..=FORMAT_VERSION).contains(&version) {
            return Err(HeaderError::UnsupportedVersion(version));
        }

        match Self::decode_fields(fields, version) {
            Err(error) if version > OLDEST_FORMAT_VERSION => bytes
                .get(RAISED_OFFSET..RAISED_OFFSET + STATIC_LEN)
                .filter(|copy| copy[..STATIC_CHECK_OFFSET] == fields[..STATIC_CHECK_OFFSET])
                .and_then(|copy| Self::decode_fields(copy, version).ok())
                .ok_or(error),
            result => result,
        }
    }

    /// The static fields `bytes` of a file of format `version`, once their
    /// check passes.
    fn decode_fields(bytes: &[u8], version: u32) -> Result<Self, HeaderError> {
        if Check::of(&[&bytes[..STATIC_CHECK_OFFSET]]) != Check::read(&bytes[STATIC_CHECK_OFFSET..])
        {
            return Err(HeaderError::Damaged("the static fields fail their check"));
        }

        let page_size = le_u32(bytes, 12);

        if !is_valid_page_size(page_size) {
            return Err(HeaderError::Damaged("the page size is not a valid one"));
        }

        let cipher = match bytes[32] {
            0 => Cipher::Plain,
            1 => Cipher::XChaCha20Poly1305,
            2 => Cipher::Xaes256Gcm,
            _ => return Err(HeaderError::Damaged("the cipher is not a known one")),
        };
        let mut file_id = [0u8; 16];

        file_id.copy_from_slice(&bytes[16..32]);

        Ok(Self {
            version,
            page_size,
            file_id,
            cipher,
        })
    }
}

/// The selector byte: which slot holds the published commit, and whether that
/// commit may not be durable yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selector {
    /// The slot holding the published commit: 0, 1 or 2.
    pub(crate) slot: usize,
    /// Set while the published commit is a deferred commit that no barrier has
    /// confirmed yet.
    pub(crate) unsynced: bool,
}

impl Selector {
    /// The selector as its byte.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a slot number is 0, 1 or 2"
    )]
    pub(crate) fn encode(&self) -> u8 {
        debug_assert!(self.slot < SLOT_COUNT);

        let slot = self.slot as u8;

        if self.unsynced { slot | 0b100 } else { slot }
    }

    /// Reads the selector byte.
    pub(crate) fn decode(byte: u8) -> Result<Self, &'static str> {
        if byte & !0b111 != 0 {
            return Err("the selector has a reserved bit set");
        }

        let slot = usize::from(byte & 0b11);

        if slot >= SLOT_COUNT {
            return Err("the selector names a slot that does not exist");
        }

        Ok(Self {
            slot,
            unsynced: byte & 0b100 != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> StaticHeader {
        StaticHeader {
            version: FORMAT_VERSION,
            page_size: 4096,
            file_id: *b"0123456789abcdef",
            cipher: Cipher::Plain,
        }
    }

    #[test]
    fn the_cipher_is_byte_32_and_an_unknown_one_is_refused() {
        let encrypted = StaticHeader {
            cipher: Cipher::XChaCha20Poly1305,
            ..header()
        };
        let mut bytes = encrypted.encode();

        assert_eq!(bytes[32], 1);
        assert_eq!(StaticHeader::decode(&bytes), Ok(encrypted));

        bytes[32] = 3;

        let check = Check::of(&[&bytes[..48]]);

        check.write(&mut bytes[48..]);

        assert!(matches!(
            StaticHeader::decode(&bytes),
            Err(HeaderError::Damaged(_))
        ));
    }

    #[test]
    fn the_static_fields_read_back_as_they_were_written() {
        assert_eq!(StaticHeader::decode(&header().encode()), Ok(header()));
    }

    #[test]
    fn the_static_layout_is_the_documented_one() {
        let bytes = header().encode();

        assert_eq!(&bytes[0..8], b"\x89DaruDB\n");
        assert_eq!(&bytes[8..12], &6u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &4096u32.to_le_bytes());
        assert_eq!(&bytes[16..32], b"0123456789abcdef");
        assert_eq!(bytes[32], 0, "the cipher of a plain file");
        assert_eq!(&bytes[33..48], &[0; 15]);
        assert_eq!(Check::read(&bytes[48..]), Check::of(&[&bytes[..48]]));
    }

    #[test]
    fn the_parts_of_page_zero_sit_where_the_specification_puts_them() {
        assert_eq!(SELECTOR_OFFSET, 64);
        assert_eq!(
            (0..SLOT_COUNT).map(slot_offset).collect::<Vec<_>>(),
            [512, 1024, 1536]
        );
        assert_eq!(slot_offset(SLOT_COUNT - 1) + SLOT_LEN, HEADER_LEN);
        assert_eq!(RAISED_OFFSET, 128);
        assert!(SELECTOR_OFFSET < RAISED_OFFSET && RAISED_OFFSET + STATIC_LEN <= slot_offset(0));
    }

    #[test]
    fn too_few_bytes_or_other_leading_bytes_are_not_a_database() {
        let bytes = header().encode();
        let mut other = bytes;

        other[0] = b'D';

        assert_eq!(
            StaticHeader::decode(&bytes[..63]),
            Err(HeaderError::NotADatabase)
        );
        assert_eq!(StaticHeader::decode(&other), Err(HeaderError::NotADatabase));
    }

    #[test]
    fn another_version_is_reported_before_anything_else_is_judged() {
        let mut bytes = header().encode();

        // Format version 1 had no static check, so it must not be the reason.
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());

        assert_eq!(
            StaticHeader::decode(&bytes),
            Err(HeaderError::UnsupportedVersion(1))
        );

        bytes[8..12].copy_from_slice(&7u32.to_le_bytes());

        assert_eq!(
            StaticHeader::decode(&bytes),
            Err(HeaderError::UnsupportedVersion(7))
        );
    }

    #[test]
    fn a_file_of_the_oldest_version_reads_as_that_version() {
        let old = StaticHeader {
            version: OLDEST_FORMAT_VERSION,
            ..header()
        };
        let bytes = old.encode();

        assert_eq!(&bytes[8..12], &5u32.to_le_bytes());
        assert_eq!(StaticHeader::decode(&bytes), Ok(old));
    }

    /// Page 0 as raising the version from 5 to 6 leaves it after its first
    /// `steps` writes: the copy, the version's byte, then the check, which a
    /// cut may leave with only its first `torn` bytes written.
    fn raising(steps: usize, torn: usize) -> Vec<u8> {
        let old = StaticHeader {
            version: OLDEST_FORMAT_VERSION,
            ..header()
        };
        let new = header().encode();
        let mut page = vec![0u8; HEADER_LEN];

        page[..STATIC_LEN].copy_from_slice(&old.encode());

        if steps >= 1 {
            page[RAISED_OFFSET..RAISED_OFFSET + STATIC_LEN].copy_from_slice(&new);
        }

        if steps >= 2 {
            page[8] = new[8];
        }

        if steps >= 3 {
            let end = STATIC_CHECK_OFFSET + torn;

            page[STATIC_CHECK_OFFSET..end].copy_from_slice(&new[STATIC_CHECK_OFFSET..end]);
        }

        page
    }

    #[test]
    fn a_cut_while_the_version_is_raised_leaves_one_version_or_the_other() {
        let old = StaticHeader {
            version: OLDEST_FORMAT_VERSION,
            ..header()
        };

        assert_eq!(StaticHeader::decode(&raising(0, 0)), Ok(old));
        assert_eq!(StaticHeader::decode(&raising(1, 0)), Ok(old));

        for torn in 0..=CHECK_LEN {
            assert_eq!(
                StaticHeader::decode(&raising(3, torn)),
                Ok(header()),
                "{torn} bytes of the check written"
            );
        }

        // Without the copy, the new version and the old check are damage.
        let mut lost = raising(2, 0);

        lost[RAISED_OFFSET..RAISED_OFFSET + STATIC_LEN].fill(0);

        assert!(matches!(
            StaticHeader::decode(&lost),
            Err(HeaderError::Damaged(_))
        ));
    }

    #[test]
    fn a_copy_of_other_fields_does_not_vouch_for_damaged_ones() {
        let mut page = raising(2, 0);

        page[20] ^= 1;

        assert!(matches!(
            StaticHeader::decode(&page),
            Err(HeaderError::Damaged(_))
        ));
    }

    #[test]
    fn a_changed_static_byte_fails_the_check() {
        let mut bytes = header().encode();

        bytes[20] ^= 1;

        assert!(matches!(
            StaticHeader::decode(&bytes),
            Err(HeaderError::Damaged(_))
        ));
    }

    #[test]
    fn page_sizes_below_4096_are_refused() {
        let mut header = header();

        header.page_size = 2048;

        assert!(matches!(
            StaticHeader::decode(&header.encode()),
            Err(HeaderError::Damaged(_))
        ));
    }

    #[test]
    fn the_selector_reads_back_and_refuses_what_it_cannot_mean() {
        for slot in 0..SLOT_COUNT {
            for unsynced in [false, true] {
                let selector = Selector { slot, unsynced };

                assert_eq!(Selector::decode(selector.encode()), Ok(selector));
            }
        }

        assert_eq!(
            Selector {
                slot: 2,
                unsynced: true
            }
            .encode(),
            0b110
        );
        assert!(Selector::decode(0b011).is_err(), "slot 3");
        assert!(Selector::decode(0b1000).is_err(), "a reserved bit");
    }
}
