//! Checks: the 16 bytes that prove a page, a record or the static fields are
//! the ones expected.
//!
//! In a plain file a check is an XXH3-128 hash, with seed 0, stored as a
//! little-endian 128-bit integer. In an encrypted file a page's check is its
//! authentication tag instead; that arrives with encryption.

use twox_hash::xxhash3_128::{RawHasher, SecretBuffer};

/// The size of a check, in bytes.
pub(crate) const CHECK_LEN: usize = 16;

/// A 16-byte check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Check(pub(crate) [u8; CHECK_LEN]);

impl Check {
    /// All zeros: the check of a null pointer and of an unused field.
    pub(crate) const ZERO: Check = Check([0; CHECK_LEN]);

    /// The XXH3-128 hash of `parts`, concatenated in order.
    pub(crate) fn of(parts: &[&[u8]]) -> Check {
        // The default seed and secret, which `XxHash3_128::new` puts on the
        // heap: hashing a page took a quarter longer with the allocation.
        let mut hasher = RawHasher::new(SecretBuffer::default());

        for part in parts {
            hasher.write(part);
        }

        Check(hasher.finish_128().to_le_bytes())
    }

    /// Reads a check from the first 16 bytes of `bytes`.
    pub(crate) fn read(bytes: &[u8]) -> Check {
        let mut check = [0; CHECK_LEN];

        check.copy_from_slice(&bytes[..CHECK_LEN]);

        Check(check)
    }

    /// Writes the check into the first 16 bytes of `bytes`.
    pub(crate) fn write(&self, bytes: &mut [u8]) {
        bytes[..CHECK_LEN].copy_from_slice(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_xxh3_128_with_seed_zero_stored_little_endian() {
        // The published XXH3-128 value of the empty input is
        // 0x99aa06d3014798d86001c324468d497f.
        let expected = 0x99aa_06d3_0147_98d8_6001_c324_468d_497f_u128.to_le_bytes();

        assert_eq!(Check::of(&[]), Check(expected));
    }

    #[test]
    fn parts_hash_as_their_concatenation() {
        assert_eq!(
            Check::of(&[b"page", b" ", b"bytes"]),
            Check::of(&[b"page bytes"])
        );
    }

    #[test]
    fn a_check_reads_back_as_it_was_written() {
        let check = Check::of(&[b"anything"]);
        let mut bytes = [0u8; 20];

        check.write(&mut bytes[2..]);

        assert_eq!(Check::read(&bytes[2..]), check);
    }
}
