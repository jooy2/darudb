//! Encryption: sealing and opening the pages of an encrypted file, and
//! wrapping its data key under the caller's key or password.
//!
//! Like `format`, nothing here reads or writes a file: every function takes
//! bytes and keys and returns bytes, and the randomness it needs is passed in,
//! so the layer above decides where it comes from and the tests can fix it.
//! `design/file-format.md`, section "Encryption", is the specification.
//!
//! Keys live in [`zeroize::Zeroizing`] buffers, and the cipher wipes its own
//! copy when it is dropped. That narrows how long a key sits in memory; it
//! cannot rule out a copy the compiler or the operating system made.

mod keys;
mod page;
mod record;

pub(crate) use keys::{DataKey, PasswordCost, Secret, Unlocker, wrap};
pub(crate) use page::PageCipher;
pub(crate) use record::RecordAuth;
