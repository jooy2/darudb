//! The keys and values of the engine's own trees: the catalog, the free tree
//! and the retained tree.
//!
//! Keys whose order matters numerically are big-endian, so that the byte order
//! the B+tree sorts by is the numeric order.

use super::node::inline_limit;
use super::pointer::{POINTER_LEN, Pointer};
use super::{FIRST_USER_TREE, le_u32, le_u64};

/// The tree id of the catalog.
pub(crate) const CATALOG_TREE: u64 = 1;

/// The tree id of the free tree.
pub(crate) const FREE_TREE: u64 = 2;

/// The tree id of the retained tree.
pub(crate) const RETAINED_TREE: u64 = 3;

/// The size of a tree descriptor, the value of a catalog entry.
pub(crate) const DESCRIPTOR_LEN: usize = 56;

/// What the catalog records about one tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TreeDescriptor {
    /// The tree's id, 16 or more.
    pub(crate) id: u64,
    /// The tree's root; null for an empty tree.
    pub(crate) root: Pointer,
    /// The number of entries in the tree.
    pub(crate) entries: u64,
}

impl TreeDescriptor {
    /// The descriptor as a catalog value.
    pub(crate) fn encode(&self) -> [u8; DESCRIPTOR_LEN] {
        let mut bytes = [0u8; DESCRIPTOR_LEN];

        bytes[0..8].copy_from_slice(&self.id.to_le_bytes());
        self.root.write(&mut bytes[8..8 + POINTER_LEN]);
        bytes[40..48].copy_from_slice(&self.entries.to_le_bytes());
        // 48..52: flags for the layers above, none yet. 52..56: reserved.

        bytes
    }

    /// Reads a catalog value.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != DESCRIPTOR_LEN {
            return Err("a catalog entry is not a tree descriptor");
        }

        let descriptor = Self {
            id: le_u64(bytes, 0),
            root: Pointer::read(&bytes[8..]),
            entries: le_u64(bytes, 40),
        };

        if descriptor.id < FIRST_USER_TREE {
            return Err("a catalog entry names a tree id reserved for the engine");
        }

        if descriptor.root.is_null() != (descriptor.entries == 0) {
            return Err("a catalog entry's root and entry count disagree");
        }

        Ok(descriptor)
    }
}

/// The free tree's key for the run starting at `start`.
pub(crate) fn free_key(start: u64) -> [u8; 8] {
    start.to_be_bytes()
}

/// The start page of a free tree key.
pub(crate) fn decode_free_key(key: &[u8]) -> Result<u64, &'static str> {
    let key: [u8; 8] = key
        .try_into()
        .map_err(|_| "a free tree key is not a page number")?;

    Ok(u64::from_be_bytes(key))
}

/// The free tree's value for a run of `len` pages.
pub(crate) fn free_value(len: u64) -> [u8; 8] {
    len.to_le_bytes()
}

/// The length of a free tree run.
pub(crate) fn decode_free_value(value: &[u8]) -> Result<u64, &'static str> {
    if value.len() != 8 {
        return Err("a free tree value is not a run length");
    }

    Ok(le_u64(value, 0))
}

/// The retained tree's key for part `sequence` of the group made by commit
/// `txn`.
pub(crate) fn retained_key(txn: u64, sequence: u32) -> [u8; 12] {
    let mut key = [0u8; 12];

    key[0..8].copy_from_slice(&txn.to_be_bytes());
    key[8..12].copy_from_slice(&sequence.to_be_bytes());

    key
}

/// The commit and the sequence number of a retained tree key.
pub(crate) fn decode_retained_key(key: &[u8]) -> Result<(u64, u32), &'static str> {
    if key.len() != 12 {
        return Err("a retained tree key is not a group key");
    }

    let mut txn = [0u8; 8];
    let mut sequence = [0u8; 4];

    txn.copy_from_slice(&key[0..8]);
    sequence.copy_from_slice(&key[8..12]);

    Ok((u64::from_be_bytes(txn), u32::from_be_bytes(sequence)))
}

/// How many 12-byte runs one retained tree value holds, so that the entry
/// stays inline.
pub(crate) fn runs_per_value(page_size: usize) -> usize {
    // An inline entry spends at most 7 bytes besides its key and value, its
    // slot and the lengths of format 5, which those of format 6 never
    // exceed, and the key is 12 bytes.
    (inline_limit(page_size) - 7 - 12) / 12
}

/// A retained tree value: runs of pages, each a first page and a length.
pub(crate) fn encode_runs(runs: &[(u64, u32)]) -> Vec<u8> {
    let mut value = Vec::with_capacity(12 * runs.len());

    for (start, len) in runs {
        value.extend_from_slice(&start.to_le_bytes());
        value.extend_from_slice(&len.to_le_bytes());
    }

    value
}

/// The runs of a retained tree value.
pub(crate) fn decode_runs(value: &[u8]) -> Result<Vec<(u64, u32)>, &'static str> {
    if value.len() % 12 != 0 {
        return Err("a retained tree value is not a list of runs");
    }

    Ok(value
        .chunks_exact(12)
        .map(|run| (le_u64(run, 0), le_u32(run, 8)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Check;
    use crate::format::node::{Cells, inline_entry_len};

    #[test]
    fn a_descriptor_reads_back_as_it_was_written() {
        let descriptor = TreeDescriptor {
            id: 16,
            root: Pointer {
                page: 4,
                txn: 2,
                check: Check::of(&[b"root"]),
            },
            entries: 12,
        };

        assert_eq!(TreeDescriptor::decode(&descriptor.encode()), Ok(descriptor));
    }

    #[test]
    fn a_descriptor_whose_root_and_count_disagree_is_refused() {
        let descriptor = TreeDescriptor {
            id: 16,
            root: Pointer::NULL,
            entries: 3,
        };

        assert!(TreeDescriptor::decode(&descriptor.encode()).is_err());
    }

    #[test]
    fn system_keys_sort_in_numeric_order() {
        assert!(free_key(255) < free_key(256));
        assert!(retained_key(1, u32::MAX) < retained_key(2, 0));
        assert_eq!(decode_free_key(&free_key(99)), Ok(99));
        assert_eq!(decode_retained_key(&retained_key(7, 3)), Ok((7, 3)));
    }

    #[test]
    fn a_full_retained_value_stays_inline() {
        let runs = vec![(1, 1); runs_per_value(4096)];
        let value = encode_runs(&runs);

        for cells in [Cells::Fixed, Cells::Varint] {
            assert!(inline_entry_len(cells, 12, value.len()) <= inline_limit(4096));
        }
        assert_eq!(decode_runs(&value), Ok(runs));
    }
}
