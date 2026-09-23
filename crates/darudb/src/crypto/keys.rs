//! The data key, the caller's key or password, and the key block that holds
//! the one wrapped under the other.
//!
//! The data key encrypts every page and never changes. The caller's secret,
//! or the Argon2id hash of a password, wraps it with XChaCha20-Poly1305, with
//! the file id as associated data, into the key block of every commit record.
//! Changing the password rewraps the data key and re-encrypts nothing.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::{KeyInit, Tag, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::format::{Kdf, KeyBlock};

/// The key that encrypts an encrypted file's pages.
pub(crate) struct DataKey(Zeroizing<[u8; 32]>);

impl DataKey {
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for DataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DataKey")
    }
}

/// What the caller supplies to open an encrypted file.
#[derive(Clone)]
pub(crate) enum Secret {
    /// A 32-byte key, used as it is.
    Key(Zeroizing<[u8; 32]>),
    /// A password, hashed with Argon2id into a key.
    Password(Zeroizing<Vec<u8>>),
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Which kind, never the secret.
        match self {
            Self::Key(_) => f.write_str("Key(..)"),
            Self::Password(_) => f.write_str("Password(..)"),
        }
    }
}

/// How much work turning a password into a key takes: the Argon2id memory,
/// iterations and parallelism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PasswordCost {
    pub(crate) memory_kib: u32,
    pub(crate) iterations: u32,
    pub(crate) parallelism: u32,
}

impl PasswordCost {
    /// 19 MiB, two passes, one lane: the lowest Argon2id setting the OWASP
    /// password storage guidance recommends. The memory is what keeps it
    /// within reach of a mobile app extension, whose whole process may be
    /// allowed little more; an application can ask for more.
    pub(crate) const DEFAULT: Self = Self {
        memory_kib: 19 * 1024,
        iterations: 2,
        parallelism: 1,
    };

    /// The most memory a key block may ask for. The hash allocates it at
    /// once, so a file must not be able to ask for more than a device has.
    const MAX_MEMORY_KIB: u32 = 1024 * 1024;
    const MAX_ITERATIONS: u32 = 1024;
    const MAX_PARALLELISM: u32 = 64;

    /// Whether Argon2id accepts this cost and it stays within the limits.
    pub(crate) fn check(&self) -> Result<(), &'static str> {
        if self.parallelism == 0 || self.parallelism > Self::MAX_PARALLELISM {
            return Err("the password hashing parallelism must be from 1 to 64");
        }

        if self.iterations == 0 || self.iterations > Self::MAX_ITERATIONS {
            return Err("the password hashing iterations must be from 1 to 1024");
        }

        if self.memory_kib < 8 * self.parallelism || self.memory_kib > Self::MAX_MEMORY_KIB {
            return Err("the password hashing memory must be from 8 KiB per lane to 1 GiB, in KiB");
        }

        Ok(())
    }
}

/// The key that wraps the data key, derived from the secret for `kdf`. `None`
/// when the secret is not of the kind the key block expects.
fn wrapping_key(secret: &Secret, kdf: &Kdf) -> Result<Option<Zeroizing<[u8; 32]>>, &'static str> {
    match (secret, kdf) {
        (Secret::Key(key), Kdf::Key) => Ok(Some(key.clone())),
        (
            Secret::Password(password),
            Kdf::Argon2id {
                memory_kib,
                iterations,
                parallelism,
                salt,
            },
        ) => {
            let cost = PasswordCost {
                memory_kib: *memory_kib,
                iterations: *iterations,
                parallelism: *parallelism,
            };

            cost.check()?;

            let params = Params::new(cost.memory_kib, cost.iterations, cost.parallelism, Some(32))
                .map_err(|_| "Argon2id refuses the password hashing cost")?;
            let mut key = Zeroizing::new([0u8; 32]);

            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password_into(password, salt, key.as_mut_slice())
                .map_err(|_| "Argon2id refuses the password or its salt")?;

            Ok(Some(key))
        }
        _ => Ok(None),
    }
}

/// Wraps `key` under `secret` for the file `file_id`. `random` supplies the
/// salt (its first 16 bytes) and the wrapping nonce (the other 24), and must
/// be fresh.
pub(crate) fn wrap(
    secret: &Secret,
    cost: PasswordCost,
    key: &DataKey,
    file_id: &[u8; 16],
    random: &[u8; 40],
) -> Result<KeyBlock, &'static str> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];

    salt.copy_from_slice(&random[..16]);
    nonce.copy_from_slice(&random[16..]);

    let kdf = match secret {
        Secret::Key(_) => Kdf::Key,
        Secret::Password(_) => Kdf::Argon2id {
            memory_kib: cost.memory_kib,
            iterations: cost.iterations,
            parallelism: cost.parallelism,
            salt,
        },
    };
    let wrapping = wrapping_key(secret, &kdf)?.ok_or("the secret does not match its kind")?;
    let mut wrapped = Zeroizing::new(*key.bytes());
    let tag = XChaCha20Poly1305::new((&*wrapping).into())
        .encrypt_inout_detached(&XNonce::from(nonce), file_id, wrapped.as_mut_slice().into())
        .map_err(|_| "the data key cannot be wrapped")?;

    Ok(KeyBlock {
        kdf,
        nonce,
        wrapped: *wrapped,
        tag: tag.into(),
    })
}

/// Tries one secret on the key blocks of a file, deriving each wrapping key
/// once: the records of a file usually share one key block, and a password
/// hash is expensive on purpose.
pub(crate) struct Unlocker<'a> {
    secret: &'a Secret,
    derived: Vec<(Kdf, Option<Zeroizing<[u8; 32]>>)>,
}

impl<'a> Unlocker<'a> {
    pub(crate) fn new(secret: &'a Secret) -> Self {
        Self {
            secret,
            derived: Vec::new(),
        }
    }

    /// The data key `block` holds, if the secret unwraps it. `Err` for a key
    /// block whose password hashing cost is impossible or too high.
    pub(crate) fn unlock(
        &mut self,
        block: &KeyBlock,
        file_id: &[u8; 16],
    ) -> Result<Option<DataKey>, &'static str> {
        let known = self.derived.iter().position(|(kdf, _)| *kdf == block.kdf);
        let index = match known {
            Some(index) => index,
            None => {
                let key = wrapping_key(self.secret, &block.kdf)?;

                self.derived.push((block.kdf, key));
                self.derived.len() - 1
            }
        };
        let Some(wrapping) = &self.derived[index].1 else {
            return Ok(None);
        };
        let mut key = Zeroizing::new(block.wrapped);
        let opened = XChaCha20Poly1305::new((&**wrapping).into()).decrypt_inout_detached(
            &XNonce::from(block.nonce),
            file_id,
            key.as_mut_slice().into(),
            &Tag::from(block.tag),
        );

        Ok(opened.is_ok().then(|| DataKey::from_bytes(*key)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap enough for tests; never for a real password.
    const CHEAP: PasswordCost = PasswordCost {
        memory_kib: 8,
        iterations: 1,
        parallelism: 1,
    };

    fn password(text: &str) -> Secret {
        Secret::Password(Zeroizing::new(text.as_bytes().to_vec()))
    }

    fn key(byte: u8) -> Secret {
        Secret::Key(Zeroizing::new([byte; 32]))
    }

    #[test]
    fn the_secret_that_wrapped_the_data_key_unwraps_it_and_no_other_does() {
        let data = DataKey::from_bytes([42; 32]);
        let file = [1; 16];

        for (right, wrong) in [(key(1), key(2)), (password("right"), password("wrong"))] {
            let block = wrap(&right, CHEAP, &data, &file, &[5; 40]).unwrap();
            let unwrapped = Unlocker::new(&right).unlock(&block, &file).unwrap();

            assert_eq!(unwrapped.map(|key| *key.bytes()), Some([42; 32]));
            assert!(
                Unlocker::new(&wrong)
                    .unlock(&block, &file)
                    .unwrap()
                    .is_none()
            );
            assert!(
                Unlocker::new(&right)
                    .unlock(&block, &[2; 16])
                    .unwrap()
                    .is_none(),
                "a key block copied into another file"
            );
        }
    }

    #[test]
    fn a_password_does_not_open_a_key_block_made_for_a_key() {
        let data = DataKey::from_bytes([42; 32]);
        let block = wrap(&key(1), CHEAP, &data, &[1; 16], &[5; 40]).unwrap();

        assert!(
            Unlocker::new(&password("x"))
                .unlock(&block, &[1; 16])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_key_block_asking_for_too_much_memory_is_refused_before_hashing() {
        let data = DataKey::from_bytes([42; 32]);
        let mut block = wrap(&password("x"), CHEAP, &data, &[1; 16], &[5; 40]).unwrap();

        if let Kdf::Argon2id { memory_kib, .. } = &mut block.kdf {
            *memory_kib = u32::MAX;
        }

        assert!(
            Unlocker::new(&password("x"))
                .unlock(&block, &[1; 16])
                .is_err()
        );
    }
}
