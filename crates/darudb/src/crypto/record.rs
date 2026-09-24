//! Authenticating the commit records of an encrypted file.
//!
//! Page 0 stays plain, because it says how to read everything else. A record
//! names the roots of the file's trees together with their tags, and those
//! tags are stored in the pages themselves, in the clear. Without a key of its
//! own, a record could be assembled by anyone who can write the file, from
//! pages that are there: the catalog of one commit with the free tree of
//! another, say, which no key holder ever committed. The record MAC rules
//! that out. Replacing page 0 with an older copy of itself is still possible,
//! as replacing the whole file is (`design/file-format.md`, "What stays
//! visible").
//!
//! The MAC is keyed BLAKE2b with a 16-byte output, over the file id, the slot
//! number and the record's first 256 bytes, under a key derived from the data
//! key. BLAKE2b is already in the build for Argon2id, and a MAC of its own
//! keeps the record independent of which cipher encrypts the pages.

use std::fmt;

use blake2::Blake2bMac;
use blake2::digest::Mac;
use blake2::digest::consts::{U16, U32};
use zeroize::Zeroizing;

use super::keys::DataKey;
use crate::format::RECORD_MAC_LEN;

/// Separates the derived key from any other use of the data key.
const KEY_PERSONA: &[u8] = b"DaruDB rec key";

/// Separates the MAC from any other keyed BLAKE2b use of the same key.
const MAC_PERSONA: &[u8] = b"DaruDB rec mac";

/// The key that authenticates an encrypted file's commit records.
pub(crate) struct RecordAuth {
    key: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for RecordAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecordAuth")
    }
}

impl RecordAuth {
    pub(crate) fn new(data_key: &DataKey) -> Self {
        let mut key = Zeroizing::new([0u8; 32]);

        // The data key and the persona have fixed, valid lengths.
        if let Ok(derive) =
            Blake2bMac::<U32>::new_with_salt_and_personal(Some(data_key.bytes()), &[], KEY_PERSONA)
        {
            key.copy_from_slice(&derive.finalize().into_bytes());
        }

        Self { key }
    }

    /// The MAC of the record whose first 256 bytes are `authenticated`, in
    /// slot `slot` of the file `file_id`.
    pub(crate) fn mac(
        &self,
        file_id: &[u8; 16],
        slot: usize,
        authenticated: &[u8],
    ) -> [u8; RECORD_MAC_LEN] {
        let mut out = [0u8; RECORD_MAC_LEN];

        if let Some(mac) = self.state(file_id, slot, authenticated) {
            out.copy_from_slice(&mac.finalize().into_bytes());
        }

        out
    }

    /// Whether `mac` is the MAC of that record, compared in constant time.
    pub(crate) fn verify(
        &self,
        file_id: &[u8; 16],
        slot: usize,
        authenticated: &[u8],
        mac: &[u8; RECORD_MAC_LEN],
    ) -> bool {
        self.state(file_id, slot, authenticated)
            .is_some_and(|state| state.verify_slice(mac).is_ok())
    }

    fn state(
        &self,
        file_id: &[u8; 16],
        slot: usize,
        authenticated: &[u8],
    ) -> Option<Blake2bMac<U16>> {
        let mut state = Blake2bMac::<U16>::new_with_salt_and_personal(
            Some(self.key.as_slice()),
            &[],
            MAC_PERSONA,
        )
        .ok()?;

        state.update(file_id);
        state.update(&u64::try_from(slot).ok()?.to_le_bytes());
        state.update(authenticated);

        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_verifies_only_as_it_was_signed() {
        let auth = RecordAuth::new(&DataKey::from_bytes([1; 32]));
        let body = [7u8; 256];
        let mac = auth.mac(&[2; 16], 1, &body);

        assert!(auth.verify(&[2; 16], 1, &body, &mac));
        assert!(!auth.verify(&[2; 16], 2, &body, &mac), "another slot");
        assert!(!auth.verify(&[3; 16], 1, &body, &mac), "another file");

        let mut changed = body;

        changed[100] ^= 1;

        assert!(!auth.verify(&[2; 16], 1, &changed, &mac), "a changed byte");
        assert!(
            !RecordAuth::new(&DataKey::from_bytes([9; 32])).verify(&[2; 16], 1, &body, &mac),
            "another data key"
        );
        assert_ne!(mac, [0; RECORD_MAC_LEN]);
    }
}
