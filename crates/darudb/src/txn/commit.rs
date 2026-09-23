//! The commit: finishing the trees, settling the allocator trees, writing the
//! pages and the record, the barrier, and publishing.
//!
//! This is a sync commit as `design/commits-and-recovery.md` specifies it:
//! the barrier is the commit point, and the selector is written afterwards to
//! publish a commit that is already durable.

use std::sync::Arc;

use super::write::WriteTransaction;
use crate::btree::{self, FinishedPage, Load, LoadedNode};
use crate::error::{Error, Result};
use crate::format::{
    CATALOG_TREE, CommitRecord, FREE_TREE, KEY_BLOCK_LEN, Pointer, RETAINED_TREE, SELECTOR_OFFSET,
    Selector, TreeDescriptor, encode_runs, free_key, free_value, retained_key, runs_per_value,
    slot_offset,
};
use crate::instance::Header;

/// How many rounds the allocator trees may take to settle. They settle in two
/// or three; running out means a bug.
const MAX_ROUNDS: usize = 64;

pub(super) fn commit(mut txn: WriteTransaction) -> Result<()> {
    txn.shared.check_usable()?;

    let loader = txn.shared.loader.clone();
    let page_size = loader.page_size();
    let mut pages = Vec::new();

    // The user trees first: each root pointer goes into the catalog.
    for (name, state) in std::mem::take(&mut txn.trees) {
        if !state.changed {
            continue;
        }

        if state.deleted {
            if state.existed {
                btree::remove(
                    &loader,
                    &mut txn.space,
                    CATALOG_TREE,
                    &mut txn.catalog,
                    name.as_bytes(),
                )?;
            }

            continue;
        }

        let root = match state.root {
            Some(root) => btree::finish(page_size, txn.txn, state.id, root, &mut pages)?,
            None => Pointer::NULL,
        };
        let descriptor = TreeDescriptor {
            id: state.id,
            root,
            entries: state.entries,
        };

        btree::insert(
            &loader,
            &mut txn.space,
            CATALOG_TREE,
            &mut txn.catalog,
            name.as_bytes(),
            &descriptor.encode(),
        )?;
    }

    settle_allocator_trees(&mut txn)?;

    let catalog = finish_root(&mut txn, CATALOG_TREE, &mut pages, |txn| txn.catalog.take())?;
    let free = finish_root(&mut txn, FREE_TREE, &mut pages, |txn| txn.free_root.take())?;
    let retained = finish_root(&mut txn, RETAINED_TREE, &mut pages, |txn| {
        txn.retained_root.take()
    })?;
    let record = CommitRecord {
        txn: txn.txn,
        durable_txn: txn.durable.txn,
        page_count: txn.space.page_count(),
        next_tree_id: txn.next_tree_id,
        catalog,
        free,
        retained,
        key_block: [0; KEY_BLOCK_LEN],
    };

    write_and_publish(&txn, pages, &record)
}

/// Brings the free tree and the retained tree in line with the transaction's
/// free space, round after round, until a round moves no page.
///
/// Changing either tree copies its pages, which takes free pages and retains
/// committed ones, which changes the trees again. A page is copied only the
/// first time a round touches it, so the rounds end quickly.
fn settle_allocator_trees(txn: &mut WriteTransaction) -> Result<()> {
    let loader = txn.shared.loader.clone();
    let page_size = loader.page_size();

    for key in std::mem::take(&mut txn.reclaimed) {
        btree::remove(
            &loader,
            &mut txn.space,
            RETAINED_TREE,
            &mut txn.retained_root,
            &key,
        )?;
    }

    // The free pages only shrink from here on, which is what lets the rounds
    // end: see `Space::settle`.
    txn.space.trim_tail();
    txn.space.settle();

    let mut written_group: Vec<Vec<u8>> = Vec::new();

    for _ in 0..MAX_ROUNDS {
        let before = txn.space.changes();
        let target = txn.space.free().clone();

        for (start, len) in &txn.free_view {
            if target.get(start) != Some(len) {
                btree::remove(
                    &loader,
                    &mut txn.space,
                    FREE_TREE,
                    &mut txn.free_root,
                    &free_key(*start),
                )?;
            }
        }

        for (start, len) in &target {
            if txn.free_view.get(start) != Some(len) {
                btree::insert(
                    &loader,
                    &mut txn.space,
                    FREE_TREE,
                    &mut txn.free_root,
                    &free_key(*start),
                    &free_value(*len),
                )?;
            }
        }

        txn.free_view = target;

        let group: Vec<Vec<u8>> = txn
            .space
            .retired_runs()
            .chunks(runs_per_value(page_size))
            .map(encode_runs)
            .collect();

        if group != written_group {
            for (sequence, value) in group.iter().enumerate() {
                btree::insert(
                    &loader,
                    &mut txn.space,
                    RETAINED_TREE,
                    &mut txn.retained_root,
                    &retained_key(txn.txn, sequence_number(sequence)?),
                    value,
                )?;
            }

            for sequence in group.len()..written_group.len() {
                btree::remove(
                    &loader,
                    &mut txn.space,
                    RETAINED_TREE,
                    &mut txn.retained_root,
                    &retained_key(txn.txn, sequence_number(sequence)?),
                )?;
            }

            written_group = group;
        }

        if txn.space.changes() == before && !txn.space.retire_set_aside() {
            return Ok(());
        }
    }

    Err(Error::Internal {
        message: "the allocator trees did not settle".to_owned(),
    })
}

fn sequence_number(sequence: usize) -> Result<u32> {
    u32::try_from(sequence).map_err(|_| Error::Internal {
        message: "a retained group too large to number".to_owned(),
    })
}

/// Encodes the pages of one of the engine's trees.
fn finish_root(
    txn: &mut WriteTransaction,
    tree: u64,
    pages: &mut Vec<FinishedPage>,
    take: impl FnOnce(&mut WriteTransaction) -> Option<btree::Child>,
) -> Result<Pointer> {
    let page_size = txn.shared.loader.page_size();

    match take(txn) {
        Some(root) => btree::finish(page_size, txn.txn, tree, root, pages),
        None => Ok(Pointer::NULL),
    }
}

/// Writes the pages and the record, issues the barrier, and publishes.
fn write_and_publish(
    txn: &WriteTransaction,
    mut pages: Vec<FinishedPage>,
    record: &CommitRecord,
) -> Result<()> {
    let shared = &txn.shared;
    let pager = &shared.pager;

    pages.sort_unstable_by_key(|page| page.page);

    for page in &pages {
        pager.write_sealed(page.page, &page.bytes)?;
    }

    // The file has to hold every page the record counts, including pages at
    // the end that were allocated and released again without being written.
    // Recovery skips a record that counts more pages than the file holds.
    let needed = record.page_count * pager.page_size() as u64;

    if pager.file_len()? < needed {
        pager.resize(record.page_count)?;
    }

    pager.write_header(&record.encode(txn.slot), slot_offset(txn.slot))?;

    // The commit point. From here on the commit survives a power cut.
    if let Err(source) = pager.sync() {
        shared.fail_sync();

        return Err(Error::SyncFailed {
            path: shared.path.clone(),
            source: Some(source),
        });
    }

    let selector = Selector {
        slot: txn.slot,
        unsynced: false,
    };

    if let Err(error) = pager.write_header(&[selector.encode()], SELECTOR_OFFSET) {
        // The commit is durable, and recovery will publish it when the file is
        // opened again; until then this process cannot tell other handles.
        shared.fail_sync();

        return Err(error);
    }

    if pager.file_len()? > needed {
        // Only free pages lie past the new end. Failing to cut them off costs
        // space, nothing else, so the commit does not fail over it.
        let _ = pager.resize(record.page_count);
    }

    let mut records = txn.header.records;

    records[txn.slot] = Some(*record);
    shared.set_header(Header { selector, records });

    for page in pages {
        shared.cache.insert(
            page.page,
            page.pointer.check,
            Arc::new(LoadedNode {
                tree: page.tree,
                txn: record.txn,
                node: page.node,
            }),
        );
    }

    Ok(())
}
