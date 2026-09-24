//! One page of an encrypted file, under the data key and the file's page
//! cipher: XChaCha20-Poly1305 or XAES-256-GCM.
//!
//! Both take a 24-byte nonce and give a 16-byte tag, so a page is laid out the
//! same way under either: the nonce sits in the page's 24-byte prefix, the
//! page header and the content are the ciphertext, the page number is the
//! associated data, and the tag takes the place of the check at the end of the
//! page.
//!
//! XAES-256-GCM is several times faster on a processor with AES and
//! carry-less multiplication instructions, and several times slower without
//! them, so a new file gets the one that suits the machine creating it
//! ([`preferred_cipher`]). A file keeps its cipher for life.

use std::fmt;

use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::consts::{U16, U24};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use xaes_256_gcm::Xaes256Gcm;

use super::keys::DataKey;
use crate::format::{Check, Cipher, PAGE_HEADER_OFFSET, check_offset};

enum Aead {
    XChaCha20Poly1305(XChaCha20Poly1305),
    // Its AES key schedules take close to a kilobyte.
    Xaes256Gcm(Box<Xaes256Gcm>),
}

/// The cipher of one encrypted file's pages.
pub(crate) struct PageCipher {
    aead: Aead,
}

impl fmt::Debug for PageCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the key.
        f.write_str("PageCipher")
    }
}

impl PageCipher {
    /// The page cipher `cipher` under `key`; `None` for a plain file.
    pub(crate) fn new(cipher: Cipher, key: &DataKey) -> Option<Self> {
        let aead = match cipher {
            Cipher::Plain => return None,
            Cipher::XChaCha20Poly1305 => {
                Aead::XChaCha20Poly1305(XChaCha20Poly1305::new(key.bytes().into()))
            }
            Cipher::Xaes256Gcm => Aead::Xaes256Gcm(Box::new(Xaes256Gcm::new(key.bytes().into()))),
        };

        Some(Self { aead })
    }

    /// Encrypts page `page` in place under `nonce`, which must never have
    /// been used before, and returns its tag: the page's check.
    pub(crate) fn seal(&self, page: u64, bytes: &mut [u8], nonce: [u8; 24]) -> Check {
        match &self.aead {
            Aead::XChaCha20Poly1305(aead) => seal(aead, page, bytes, nonce),
            Aead::Xaes256Gcm(aead) => seal(aead.as_ref(), page, bytes, nonce),
        }
    }

    /// Decrypts page `page` in place and returns its tag, if the page is the
    /// one the key encrypted for that page number. `None` for anything else:
    /// a changed byte, a page from another place in the file, another key.
    pub(crate) fn open(&self, page: u64, bytes: &mut [u8]) -> Option<Check> {
        match &self.aead {
            Aead::XChaCha20Poly1305(aead) => open(aead, page, bytes),
            Aead::Xaes256Gcm(aead) => open(aead.as_ref(), page, bytes),
        }
    }
}

/// The page cipher for a new file on this machine: XAES-256-GCM where the
/// processor has AES and carry-less multiplication instructions,
/// XChaCha20-Poly1305 elsewhere.
pub(crate) fn preferred_cipher() -> Cipher {
    if aes_instructions() {
        Cipher::Xaes256Gcm
    } else {
        Cipher::XChaCha20Poly1305
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn aes_instructions() -> bool {
    std::arch::is_x86_feature_detected!("aes") && std::arch::is_x86_feature_detected!("pclmulqdq")
}

// On AArch64, `aes` covers the AES instructions and the polynomial
// multiplication GCM needs.
#[cfg(target_arch = "aarch64")]
fn aes_instructions() -> bool {
    std::arch::is_aarch64_feature_detected!("aes")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
fn aes_instructions() -> bool {
    false
}

fn seal<A>(aead: &A, page: u64, bytes: &mut [u8], nonce: [u8; 24]) -> Check
where
    A: AeadInOut<NonceSize = U24, TagSize = U16>,
{
    let end = check_offset(bytes.len());
    let (prefix, rest) = bytes.split_at_mut(PAGE_HEADER_OFFSET);
    let (body, stored) = rest.split_at_mut(end - PAGE_HEADER_OFFSET);

    prefix.copy_from_slice(&nonce);

    // Encryption fails only for a message far longer than a page.
    let tag =
        match aead.encrypt_inout_detached(&Array::from(nonce), &page.to_le_bytes(), body.into()) {
            Ok(tag) => tag,
            Err(_) => unreachable!("a page is far shorter than the cipher's limit"),
        };

    stored.copy_from_slice(&tag);

    Check(tag.into())
}

fn open<A>(aead: &A, page: u64, bytes: &mut [u8]) -> Option<Check>
where
    A: AeadInOut<NonceSize = U24, TagSize = U16>,
{
    let end = check_offset(bytes.len());
    let (prefix, rest) = bytes.split_at_mut(PAGE_HEADER_OFFSET);
    let (body, stored) = rest.split_at_mut(end - PAGE_HEADER_OFFSET);
    let nonce = Array::try_from(&*prefix).ok()?;
    let tag = Array::try_from(&*stored).ok()?;

    aead.decrypt_inout_detached(&nonce, &page.to_le_bytes(), body.into(), &tag)
        .ok()?;

    Some(Check(tag.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIPHERS: [Cipher; 2] = [Cipher::XChaCha20Poly1305, Cipher::Xaes256Gcm];

    fn cipher(kind: Cipher, byte: u8) -> PageCipher {
        PageCipher::new(kind, &DataKey::from_bytes([byte; 32])).unwrap()
    }

    fn page() -> Vec<u8> {
        let mut bytes = vec![0u8; 4096];

        bytes[24..56].copy_from_slice(&[9; 32]);
        bytes[56..4080].fill(0xAB);

        bytes
    }

    #[test]
    fn a_sealed_page_opens_to_what_it_was_and_hides_it_meanwhile() {
        for kind in CIPHERS {
            let mut bytes = page();
            let check = cipher(kind, 1).seal(7, &mut bytes, [3; 24]);

            assert_eq!(&bytes[..24], &[3; 24], "the nonce is the prefix");
            assert_eq!(&bytes[4080..], &check.0, "the tag is the check");
            assert!(!bytes.windows(8).any(|window| window == [0xAB; 8]));
            assert_eq!(cipher(kind, 1).open(7, &mut bytes), Some(check));
            assert_eq!(bytes[24..4080], page()[24..4080]);
        }
    }

    #[test]
    fn a_changed_byte_a_moved_page_or_another_key_does_not_open() {
        for kind in CIPHERS {
            let mut sealed = page();

            cipher(kind, 1).seal(7, &mut sealed, [3; 24]);

            for at in [0, 30, 2000, 4095] {
                let mut changed = sealed.clone();

                changed[at] ^= 1;

                assert_eq!(cipher(kind, 1).open(7, &mut changed), None, "byte {at}");
            }

            assert_eq!(
                cipher(kind, 1).open(8, &mut sealed.clone()),
                None,
                "another page number"
            );
            assert_eq!(
                cipher(kind, 2).open(7, &mut sealed.clone()),
                None,
                "another key"
            );
        }
    }

    #[test]
    fn the_two_ciphers_do_not_open_each_others_pages() {
        let mut sealed = page();

        cipher(Cipher::Xaes256Gcm, 1).seal(7, &mut sealed, [3; 24]);

        assert_eq!(
            cipher(Cipher::XChaCha20Poly1305, 1).open(7, &mut sealed),
            None
        );
    }

    #[test]
    fn the_same_page_sealed_twice_differs_under_another_nonce() {
        for kind in CIPHERS {
            let mut first = page();
            let mut second = page();

            cipher(kind, 1).seal(7, &mut first, [3; 24]);
            cipher(kind, 1).seal(7, &mut second, [4; 24]);

            assert_ne!(first[24..], second[24..]);
        }
    }

    #[test]
    fn xaes_256_gcm_is_the_one_the_c2sp_specification_defines() {
        // The first test vector of the C2SP XAES-256-GCM specification.
        let aead = Xaes256Gcm::new(&[1u8; 32].into());
        let mut message = *b"XAES-256-GCM";
        let tag = aead
            .encrypt_inout_detached(
                &Array::from(*b"ABCDEFGHIJKLMNOPQRSTUVWX"),
                b"",
                message.as_mut_slice().into(),
            )
            .unwrap();

        assert_eq!(
            message,
            [
                0xce, 0x54, 0x6e, 0xf6, 0x3c, 0x9c, 0xc6, 0x07, 0x65, 0x92, 0x36, 0x09
            ]
        );
        assert_eq!(
            tag.as_slice(),
            [
                0xb3, 0x3a, 0x9a, 0x19, 0x74, 0xe9, 0x6e, 0x52, 0xda, 0xf2, 0xfc, 0xf7, 0x07, 0x5e,
                0x22, 0x71
            ]
        );
    }
}
