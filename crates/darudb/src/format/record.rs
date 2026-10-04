//! Commit records: the fixed-size description of one commit, kept in a slot
//! of page 0.
//!
//! | Offset | Size | Field                                   |
//! | ------ | ---- | --------------------------------------- |
//! | 0      | 8    | Transaction id                          |
//! | 8      | 8    | Durable transaction id                  |
//! | 16     | 8    | Page count                              |
//! | 24     | 8    | Next tree id                            |
//! | 32     | 32   | Catalog root                            |
//! | 64     | 32   | Free tree root                          |
//! | 96     | 32   | Retained tree root                      |
//! | 128    | 128  | Key block, zeros in a plain file        |
//! | 256    | 16   | Record MAC, zeros in a plain file       |
//! | 272    | 8    | When the unsynced window opened         |
//! | 280    | 8    | Pages the unsynced window wrote         |
//! | 288    | 208  | Reserved                                |
//! | 496    | 16   | Record check                            |
//!
//! The record MAC covers bytes 0 to 255 under a key only the data key's
//! holder has; `crypto` computes it, and this module only places it. The
//! window's two fields lie outside it: they only tell a writer when to issue
//! a barrier, and a writer that cannot believe them issues one sooner.

use super::check::{CHECK_LEN, Check};
use super::pointer::{POINTER_LEN, Pointer};
use super::{FIRST_USER_TREE, le_u64};

/// The size of a commit record, in bytes.
pub(crate) const RECORD_LEN: usize = 512;

/// The size of the key block, in bytes.
pub(crate) const KEY_BLOCK_LEN: usize = 128;

/// The size of the record MAC, in bytes.
pub(crate) const RECORD_MAC_LEN: usize = 16;

/// How many bytes at the start of a record the record MAC covers.
pub(crate) const AUTHENTICATED_LEN: usize = 256;

/// Every transaction id is below this, 2^62 − 64, so that the lock byte of
/// every snapshot, at 2^62 + 64 + its transaction id, fits in a signed 64-bit
/// file offset (`design/locking.md`). A million commits a second would take
/// more than 100,000 years to reach it; only a damaged or forged record can.
pub(crate) const TXN_LIMIT: u64 = (1 << 62) - 64;

const CATALOG_OFFSET: usize = 32;
const FREE_OFFSET: usize = CATALOG_OFFSET + POINTER_LEN;
const RETAINED_OFFSET: usize = FREE_OFFSET + POINTER_LEN;
const KEY_BLOCK_OFFSET: usize = 128;
const RECORD_MAC_OFFSET: usize = AUTHENTICATED_LEN;
const WINDOW_OFFSET: usize = RECORD_MAC_OFFSET + RECORD_MAC_LEN;
const RECORD_CHECK_OFFSET: usize = RECORD_LEN - CHECK_LEN;

/// One commit, as a slot records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommitRecord {
    /// The commit's transaction id, 1 or more.
    pub(crate) txn: u64,
    /// The newest commit known to be durable when this record was written.
    pub(crate) durable_txn: u64,
    /// The number of pages in the file, page 0 included.
    pub(crate) page_count: u64,
    /// The id the next new tree will receive.
    pub(crate) next_tree_id: u64,
    /// The root of the catalog.
    pub(crate) catalog: Pointer,
    /// The root of the free tree.
    pub(crate) free: Pointer,
    /// The root of the retained tree.
    pub(crate) retained: Pointer,
    /// The wrapped data key of an encrypted file; zeros in a plain one.
    pub(crate) key_block: [u8; KEY_BLOCK_LEN],
    /// The record MAC of an encrypted file; zeros in a plain one.
    pub(crate) mac: [u8; RECORD_MAC_LEN],
    /// The unsynced window of a deferred commit, as far as it reached with
    /// this commit; zeros for a sync commit.
    pub(crate) window: WindowMark,
}

/// The unsynced window a deferred commit belongs to, recorded so that a
/// writer in another process goes on counting it where this one left off
/// (`design/commits-and-recovery.md`, "The unsynced window is kept short").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WindowMark {
    /// When the window's first deferred commit was made, in microseconds since
    /// the Unix epoch by the system clock; 0 when that is not known.
    pub(crate) opened_at: u64,
    /// The pages the window's commits wrote, each once as far as the writer
    /// could tell: a writer counts a page again when another process's commit
    /// wrote it first.
    pub(crate) pages: u64,
}

impl CommitRecord {
    /// The record of a new database's first commit: empty trees, one page.
    pub(crate) fn first() -> Self {
        Self {
            txn: 1,
            durable_txn: 0,
            page_count: 1,
            next_tree_id: FIRST_USER_TREE,
            catalog: Pointer::NULL,
            free: Pointer::NULL,
            retained: Pointer::NULL,
            key_block: [0; KEY_BLOCK_LEN],
            mac: [0; RECORD_MAC_LEN],
            window: WindowMark::default(),
        }
    }

    /// The bytes the record MAC covers: every field and the key block.
    pub(crate) fn authenticated(&self) -> [u8; AUTHENTICATED_LEN] {
        let mut bytes = [0u8; AUTHENTICATED_LEN];

        bytes[0..8].copy_from_slice(&self.txn.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.durable_txn.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.page_count.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.next_tree_id.to_le_bytes());
        self.catalog.write(&mut bytes[CATALOG_OFFSET..]);
        self.free.write(&mut bytes[FREE_OFFSET..]);
        self.retained.write(&mut bytes[RETAINED_OFFSET..]);
        bytes[KEY_BLOCK_OFFSET..KEY_BLOCK_OFFSET + KEY_BLOCK_LEN].copy_from_slice(&self.key_block);

        bytes
    }

    /// The record as the bytes of slot `slot`. The slot number is mixed into
    /// the record check, so a record in the wrong slot fails it.
    pub(crate) fn encode(&self, slot: usize) -> [u8; RECORD_LEN] {
        let mut bytes = [0u8; RECORD_LEN];

        bytes[..AUTHENTICATED_LEN].copy_from_slice(&self.authenticated());
        bytes[RECORD_MAC_OFFSET..RECORD_MAC_OFFSET + RECORD_MAC_LEN].copy_from_slice(&self.mac);
        bytes[WINDOW_OFFSET..WINDOW_OFFSET + 8]
            .copy_from_slice(&self.window.opened_at.to_le_bytes());
        bytes[WINDOW_OFFSET + 8..WINDOW_OFFSET + 16]
            .copy_from_slice(&self.window.pages.to_le_bytes());
        record_check(slot, &bytes).write(&mut bytes[RECORD_CHECK_OFFSET..]);

        bytes
    }

    /// Reads the record in slot `slot`.
    ///
    /// Returns `Ok(None)` for an empty slot, and an error for a record that
    /// fails its check or whose fields contradict each other.
    pub(crate) fn decode(slot: usize, bytes: &[u8]) -> Result<Option<Self>, &'static str> {
        let bytes = &bytes[..RECORD_LEN];

        if bytes[0..8] == [0; 8] {
            return Ok(None);
        }

        if record_check(slot, bytes) != Check::read(&bytes[RECORD_CHECK_OFFSET..]) {
            return Err("the record fails its check");
        }

        let mut key_block = [0u8; KEY_BLOCK_LEN];
        let mut mac = [0u8; RECORD_MAC_LEN];

        key_block.copy_from_slice(&bytes[KEY_BLOCK_OFFSET..KEY_BLOCK_OFFSET + KEY_BLOCK_LEN]);
        mac.copy_from_slice(&bytes[RECORD_MAC_OFFSET..RECORD_MAC_OFFSET + RECORD_MAC_LEN]);

        let record = Self {
            txn: le_u64(bytes, 0),
            durable_txn: le_u64(bytes, 8),
            page_count: le_u64(bytes, 16),
            next_tree_id: le_u64(bytes, 24),
            catalog: Pointer::read(&bytes[CATALOG_OFFSET..]),
            free: Pointer::read(&bytes[FREE_OFFSET..]),
            retained: Pointer::read(&bytes[RETAINED_OFFSET..]),
            key_block,
            mac,
            window: WindowMark {
                opened_at: le_u64(bytes, WINDOW_OFFSET),
                pages: le_u64(bytes, WINDOW_OFFSET + 8),
            },
        };

        record.validate()?;

        Ok(Some(record))
    }

    /// The consistency rules of `design/file-format.md`, beyond the check.
    fn validate(&self) -> Result<(), &'static str> {
        if self.txn >= TXN_LIMIT {
            return Err("the record's transaction id is past the last one a file may use");
        }

        if self.durable_txn >= self.txn {
            return Err("the record's durable transaction is not older than the record");
        }

        if self.page_count == 0 {
            return Err("the record's page count leaves out the header");
        }

        if self.next_tree_id < FIRST_USER_TREE {
            return Err("the record's next tree id is among the reserved ones");
        }

        for root in [&self.catalog, &self.free, &self.retained] {
            if root.is_null() {
                continue;
            }

            if root.page >= self.page_count || root.txn > self.txn || root.txn == 0 {
                return Err("a root pointer lies outside the commit");
            }
        }

        Ok(())
    }
}

/// The record check: XXH3-128 of the slot number, then the record's first 496
/// bytes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a slot number is 0, 1 or 2"
)]
fn record_check(slot: usize, bytes: &[u8]) -> Check {
    debug_assert!(slot < super::SLOT_COUNT);

    let slot = [slot as u8];

    Check::of(&[&slot, &bytes[..RECORD_CHECK_OFFSET]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> CommitRecord {
        CommitRecord {
            txn: 9,
            durable_txn: 8,
            page_count: 40,
            next_tree_id: 17,
            catalog: Pointer {
                page: 3,
                txn: 9,
                check: Check::of(&[b"catalog"]),
            },
            free: Pointer::NULL,
            retained: Pointer {
                page: 39,
                txn: 5,
                check: Check::of(&[b"retained"]),
            },
            key_block: [0; KEY_BLOCK_LEN],
            mac: [0; RECORD_MAC_LEN],
            window: WindowMark {
                opened_at: 1_800_000_000_000_000,
                pages: 31,
            },
        }
    }

    #[test]
    fn a_record_reads_back_as_it_was_written() {
        for slot in 0..3 {
            assert_eq!(
                CommitRecord::decode(slot, &record().encode(slot)),
                Ok(Some(record()))
            );
        }
    }

    #[test]
    fn the_layout_is_the_documented_one() {
        let bytes = record().encode(1);

        assert_eq!(&bytes[0..8], &9u64.to_le_bytes());
        assert_eq!(&bytes[8..16], &8u64.to_le_bytes());
        assert_eq!(&bytes[16..24], &40u64.to_le_bytes());
        assert_eq!(&bytes[24..32], &17u64.to_le_bytes());
        assert_eq!(Pointer::read(&bytes[32..]), record().catalog);
        assert_eq!(Pointer::read(&bytes[64..]), Pointer::NULL);
        assert_eq!(Pointer::read(&bytes[96..]), record().retained);
        assert_eq!(&bytes[128..272], &[0; 144]);
        assert_eq!(&bytes[272..280], &1_800_000_000_000_000u64.to_le_bytes());
        assert_eq!(&bytes[280..288], &31u64.to_le_bytes());
        assert_eq!(&bytes[288..496], &[0; 208]);

        let mut signed = record();

        signed.mac = [5; RECORD_MAC_LEN];

        let bytes = signed.encode(1);

        assert_eq!(&bytes[256..272], &[5; 16], "the record MAC");
        assert_eq!(&bytes[..256], &signed.authenticated());
        signed.window = WindowMark::default();
        assert_eq!(
            signed.authenticated(),
            record().authenticated(),
            "the window lies outside the MAC"
        );
        signed.window = record().window;
        assert_eq!(CommitRecord::decode(1, &bytes), Ok(Some(signed)));
    }

    #[test]
    fn an_all_zero_slot_is_empty() {
        assert_eq!(CommitRecord::decode(0, &[0; RECORD_LEN]), Ok(None));
    }

    #[test]
    fn a_record_in_another_slot_fails_its_check() {
        assert!(CommitRecord::decode(2, &record().encode(0)).is_err());
    }

    #[test]
    fn a_changed_byte_fails_the_check() {
        let mut bytes = record().encode(0);

        bytes[17] ^= 1;

        assert!(CommitRecord::decode(0, &bytes).is_err());
    }

    #[test]
    fn contradictory_fields_are_refused_even_with_a_valid_check() {
        let mut stale_durable = record();
        let mut root_past_the_end = record();
        let mut root_from_the_future = record();
        let mut past_the_last_id = record();

        stale_durable.durable_txn = stale_durable.txn;
        root_past_the_end.catalog.page = root_past_the_end.page_count;
        root_from_the_future.catalog.txn = root_from_the_future.txn + 1;
        past_the_last_id.txn = TXN_LIMIT;

        for bad in [
            stale_durable,
            root_past_the_end,
            root_from_the_future,
            past_the_last_id,
        ] {
            assert!(CommitRecord::decode(0, &bad.encode(0)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_first_record_is_a_valid_one() {
        let first = CommitRecord::first();

        assert_eq!(CommitRecord::decode(0, &first.encode(0)), Ok(Some(first)));
    }
}
