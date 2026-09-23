//! Pointers: how a record or a branch refers to another page.
//!
//! A pointer names the page, the transaction that wrote it, and its check.
//! Every page is verified against the pointer that led to it, and the
//! transaction id is what lets recovery skip the pages written before a
//! commit's durable transaction without reading them.

use super::check::{CHECK_LEN, Check};
use super::le_u64;

/// The size of an encoded pointer, in bytes.
pub(crate) const POINTER_LEN: usize = 32;

/// A reference to a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Pointer {
    /// The page number. 0 means "no page".
    pub(crate) page: u64,
    /// The transaction id of the commit that wrote the page.
    pub(crate) txn: u64,
    /// The page's check.
    pub(crate) check: Check,
}

impl Pointer {
    /// The pointer to nothing: the root of an empty tree.
    pub(crate) const NULL: Pointer = Pointer {
        page: 0,
        txn: 0,
        check: Check::ZERO,
    };

    /// Whether this is the null pointer.
    pub(crate) fn is_null(&self) -> bool {
        self.page == 0
    }

    /// Writes the pointer into the first 32 bytes of `bytes`.
    pub(crate) fn write(&self, bytes: &mut [u8]) {
        bytes[0..8].copy_from_slice(&self.page.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.txn.to_le_bytes());
        self.check.write(&mut bytes[16..16 + CHECK_LEN]);
    }

    /// Reads a pointer from the first 32 bytes of `bytes`.
    pub(crate) fn read(bytes: &[u8]) -> Pointer {
        Pointer {
            page: le_u64(bytes, 0),
            txn: le_u64(bytes, 8),
            check: Check::read(&bytes[16..]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_reads_back_as_it_was_written() {
        let pointer = Pointer {
            page: 7,
            txn: 42,
            check: Check::of(&[b"page seven"]),
        };
        let mut bytes = [0u8; POINTER_LEN];

        pointer.write(&mut bytes);

        assert_eq!(Pointer::read(&bytes), pointer);
    }

    #[test]
    fn the_layout_is_the_documented_one() {
        let check = Check::of(&[b"x"]);
        let mut bytes = [0u8; POINTER_LEN];

        Pointer {
            page: 0x0102,
            txn: 0x0304,
            check,
        }
        .write(&mut bytes);

        assert_eq!(&bytes[0..8], &0x0102u64.to_le_bytes());
        assert_eq!(&bytes[8..16], &0x0304u64.to_le_bytes());
        assert_eq!(&bytes[16..32], &check.0);
    }

    #[test]
    fn the_null_pointer_is_all_zeros() {
        let mut bytes = [0xFFu8; POINTER_LEN];

        Pointer::NULL.write(&mut bytes);

        assert_eq!(bytes, [0; POINTER_LEN]);
        assert!(Pointer::read(&bytes).is_null());
    }
}
