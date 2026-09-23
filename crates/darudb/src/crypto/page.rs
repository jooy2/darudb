//! One page of an encrypted file: XChaCha20-Poly1305 under the data key.
//!
//! The nonce sits in the page's 24-byte prefix, the page header and the
//! content are the ciphertext, the page number is the associated data, and
//! the tag takes the place of the check at the end of the page.

use std::fmt;

use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::{KeyInit, Tag, XChaCha20Poly1305, XNonce};

use super::keys::DataKey;
use crate::format::{Check, PAGE_HEADER_OFFSET, check_offset};

/// The cipher of one encrypted file's pages.
pub(crate) struct PageCipher {
    aead: XChaCha20Poly1305,
}

impl fmt::Debug for PageCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the key.
        f.write_str("PageCipher")
    }
}

impl PageCipher {
    pub(crate) fn new(key: &DataKey) -> Self {
        Self {
            aead: XChaCha20Poly1305::new(key.bytes().into()),
        }
    }

    /// Encrypts page `page` in place under `nonce`, which must never have
    /// been used before, and returns its tag: the page's check.
    pub(crate) fn seal(&self, page: u64, bytes: &mut [u8], nonce: [u8; 24]) -> Check {
        let end = check_offset(bytes.len());
        let (prefix, rest) = bytes.split_at_mut(PAGE_HEADER_OFFSET);
        let (body, stored) = rest.split_at_mut(end - PAGE_HEADER_OFFSET);

        prefix.copy_from_slice(&nonce);

        // Encryption fails only for a message longer than 256 GiB.
        let tag = match self.aead.encrypt_inout_detached(
            &XNonce::from(nonce),
            &page.to_le_bytes(),
            body.into(),
        ) {
            Ok(tag) => tag,
            Err(_) => unreachable!("a page is far shorter than the cipher's limit"),
        };

        stored.copy_from_slice(&tag);

        Check(tag.into())
    }

    /// Decrypts page `page` in place and returns its tag, if the page is the
    /// one the key encrypted for that page number. `None` for anything else:
    /// a changed byte, a page from another place in the file, another key.
    pub(crate) fn open(&self, page: u64, bytes: &mut [u8]) -> Option<Check> {
        let end = check_offset(bytes.len());
        let (prefix, rest) = bytes.split_at_mut(PAGE_HEADER_OFFSET);
        let (body, stored) = rest.split_at_mut(end - PAGE_HEADER_OFFSET);
        let nonce = XNonce::try_from(&*prefix).ok()?;
        let tag = Tag::try_from(&*stored).ok()?;

        self.aead
            .decrypt_inout_detached(&nonce, &page.to_le_bytes(), body.into(), &tag)
            .ok()?;

        Some(Check(tag.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(byte: u8) -> PageCipher {
        PageCipher::new(&DataKey::from_bytes([byte; 32]))
    }

    fn page() -> Vec<u8> {
        let mut bytes = vec![0u8; 4096];

        bytes[24..56].copy_from_slice(&[9; 32]);
        bytes[56..4080].fill(0xAB);

        bytes
    }

    #[test]
    fn a_sealed_page_opens_to_what_it_was_and_hides_it_meanwhile() {
        let mut bytes = page();
        let check = cipher(1).seal(7, &mut bytes, [3; 24]);

        assert_eq!(&bytes[..24], &[3; 24], "the nonce is the prefix");
        assert_eq!(&bytes[4080..], &check.0, "the tag is the check");
        assert!(!bytes.windows(8).any(|window| window == [0xAB; 8]));
        assert_eq!(cipher(1).open(7, &mut bytes), Some(check));
        assert_eq!(bytes[24..4080], page()[24..4080]);
    }

    #[test]
    fn a_changed_byte_a_moved_page_or_another_key_does_not_open() {
        let mut sealed = page();

        cipher(1).seal(7, &mut sealed, [3; 24]);

        for at in [0, 30, 2000, 4095] {
            let mut changed = sealed.clone();

            changed[at] ^= 1;

            assert_eq!(cipher(1).open(7, &mut changed), None, "byte {at}");
        }

        assert_eq!(
            cipher(1).open(8, &mut sealed.clone()),
            None,
            "another page number"
        );
        assert_eq!(cipher(2).open(7, &mut sealed.clone()), None, "another key");
    }

    #[test]
    fn the_same_page_sealed_twice_differs_under_another_nonce() {
        let mut first = page();
        let mut second = page();

        cipher(1).seal(7, &mut first, [3; 24]);
        cipher(1).seal(7, &mut second, [4; 24]);

        assert_ne!(first[24..], second[24..]);
    }
}
