//! The key block: how an encrypted file's data key is stored, wrapped, in
//! every commit record.
//!
//! | Offset | Size | Field                                                   |
//! | ------ | ---- | ------------------------------------------------------- |
//! | 0      | 1    | KDF: 0 for a caller's 32-byte key, 1 for Argon2id       |
//! | 1      | 3    | Reserved                                                |
//! | 4      | 4    | Argon2id memory, in KiB                                 |
//! | 8      | 4    | Argon2id iterations                                     |
//! | 12     | 4    | Argon2id parallelism                                    |
//! | 16     | 16   | Salt                                                    |
//! | 32     | 24   | Wrapping nonce                                          |
//! | 56     | 32   | Wrapped data key                                        |
//! | 88     | 16   | Wrapping tag                                            |
//! | 104    | 24   | Reserved                                                |
//!
//! A plain file's key block is all zeros. `design/file-format.md` is the
//! specification.

use super::le_u32;
use super::record::KEY_BLOCK_LEN;

/// How the key that wraps the data key comes from what the caller supplies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kdf {
    /// The caller's 32-byte key is the wrapping key.
    Key,
    /// The wrapping key is the Argon2id hash of the caller's password.
    Argon2id {
        memory_kib: u32,
        iterations: u32,
        parallelism: u32,
        salt: [u8; 16],
    },
}

/// The data key, wrapped, and how to derive the key that unwraps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyBlock {
    pub(crate) kdf: Kdf,
    pub(crate) nonce: [u8; 24],
    pub(crate) wrapped: [u8; 32],
    pub(crate) tag: [u8; 16],
}

impl KeyBlock {
    pub(crate) fn encode(&self) -> [u8; KEY_BLOCK_LEN] {
        let mut bytes = [0u8; KEY_BLOCK_LEN];

        match self.kdf {
            Kdf::Key => bytes[0] = 0,
            Kdf::Argon2id {
                memory_kib,
                iterations,
                parallelism,
                salt,
            } => {
                bytes[0] = 1;
                bytes[4..8].copy_from_slice(&memory_kib.to_le_bytes());
                bytes[8..12].copy_from_slice(&iterations.to_le_bytes());
                bytes[12..16].copy_from_slice(&parallelism.to_le_bytes());
                bytes[16..32].copy_from_slice(&salt);
            }
        }

        bytes[32..56].copy_from_slice(&self.nonce);
        bytes[56..88].copy_from_slice(&self.wrapped);
        bytes[88..104].copy_from_slice(&self.tag);

        bytes
    }

    /// Reads a key block. `None` for the all-zero block of a plain file.
    pub(crate) fn decode(bytes: &[u8; KEY_BLOCK_LEN]) -> Result<Option<Self>, &'static str> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Ok(None);
        }

        let kdf = match bytes[0] {
            0 => Kdf::Key,
            1 => {
                let mut salt = [0u8; 16];

                salt.copy_from_slice(&bytes[16..32]);

                Kdf::Argon2id {
                    memory_kib: le_u32(bytes, 4),
                    iterations: le_u32(bytes, 8),
                    parallelism: le_u32(bytes, 12),
                    salt,
                }
            }
            _ => return Err("the key block names a key derivation this build does not know"),
        };
        let mut block = Self {
            kdf,
            nonce: [0; 24],
            wrapped: [0; 32],
            tag: [0; 16],
        };

        block.nonce.copy_from_slice(&bytes[32..56]);
        block.wrapped.copy_from_slice(&bytes[56..88]);
        block.tag.copy_from_slice(&bytes[88..104]);

        Ok(Some(block))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_block_reads_back_at_the_documented_offsets() {
        let block = KeyBlock {
            kdf: Kdf::Argon2id {
                memory_kib: 65536,
                iterations: 3,
                parallelism: 1,
                salt: [5; 16],
            },
            nonce: [6; 24],
            wrapped: [7; 32],
            tag: [8; 16],
        };
        let bytes = block.encode();

        assert_eq!(bytes[0], 1);
        assert_eq!(&bytes[4..8], &65536u32.to_le_bytes());
        assert_eq!(&bytes[16..32], &[5; 16]);
        assert_eq!(&bytes[32..56], &[6; 24]);
        assert_eq!(&bytes[56..88], &[7; 32]);
        assert_eq!(&bytes[88..104], &[8; 16]);
        assert_eq!(&bytes[104..], &[0; 24]);
        assert_eq!(KeyBlock::decode(&bytes), Ok(Some(block)));
    }

    #[test]
    fn a_plain_file_has_no_key_block_and_an_unknown_kdf_is_refused() {
        assert_eq!(KeyBlock::decode(&[0; KEY_BLOCK_LEN]), Ok(None));

        let mut bytes = [0u8; KEY_BLOCK_LEN];

        bytes[0] = 2;
        bytes[40] = 1;

        assert!(KeyBlock::decode(&bytes).is_err());
    }
}
