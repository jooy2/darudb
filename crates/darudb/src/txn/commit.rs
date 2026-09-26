//! The commit: finishing the trees, settling the allocator trees, writing the
//! pages and the record, the barrier, and publishing.
//!
//! `design/commits-and-recovery.md` specifies both kinds. A sync commit's
//! barrier is its commit point, and the selector is written afterwards to
//! publish a commit that is already durable. A deferred commit is published
//! without a barrier, with the selector's unsynced bit set, and becomes durable
//! at the next barrier.

use std::sync::Arc;

use super::write::WriteTransaction;
use crate::btree::{self, FinishedPage, Load};
use crate::error::{Error, Result};
use crate::format::{
    CATALOG_TREE, CommitRecord, FREE_TREE, Pointer, RECORD_MAC_LEN, RETAINED_TREE, SELECTOR_OFFSET,
    Selector, TreeDescriptor, encode_runs, free_key, free_value, retained_key, runs_per_value,
    slot_offset,
};
use crate::instance::Header;
use crate::space::YoungPart;

/// How many rounds the allocator trees may take to settle. They settle in two
/// or three; running out means a bug.
const MAX_ROUNDS: usize = 64;

/// How a commit is made durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Durability {
    /// Durable when the commit returns: one barrier.
    Sync,
    /// Published at once, durable at the next barrier.
    Deferred,
}

pub(super) fn commit(mut txn: WriteTransaction, durability: Durability) -> Result<()> {
    txn.shared.check_owner()?;
    txn.shared.check_usable()?;

    let loader = txn.shared.loader.clone();
    let pager = Arc::clone(&txn.shared.pager);
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
            Some(root) => btree::finish(&pager, txn.txn, state.id, root, &mut pages)?,
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
            true,
        )?;
    }

    settle_allocator_trees(&mut txn)?;

    let catalog = finish_root(&mut txn, CATALOG_TREE, &mut pages, |txn| txn.catalog.take())?;
    let free = finish_root(&mut txn, FREE_TREE, &mut pages, |txn| txn.free_root.take())?;
    let retained = finish_root(&mut txn, RETAINED_TREE, &mut pages, |txn| {
        txn.retained_root.take()
    })?;
    let mut record = CommitRecord {
        txn: txn.txn,
        durable_txn: txn.durable.txn,
        page_count: txn.space.page_count(),
        next_tree_id: txn.next_tree_id,
        catalog,
        free,
        retained,
        key_block: txn.key_block,
        mac: [0; RECORD_MAC_LEN],
    };

    txn.shared.sign_record(txn.slot, &mut record);

    write_and_publish(&mut txn, pages, &record, durability)
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
        let changes = txn.space.free_tree_changes();

        // Removals first, as the free pages only shrink from here on.
        for (start, _) in changes.iter().filter(|(_, len)| len.is_none()) {
            btree::remove(
                &loader,
                &mut txn.space,
                FREE_TREE,
                &mut txn.free_root,
                &free_key(*start),
            )?;
        }

        for (start, len) in &changes {
            if let Some(len) = len {
                btree::insert(
                    &loader,
                    &mut txn.space,
                    FREE_TREE,
                    &mut txn.free_root,
                    &free_key(*start),
                    &free_value(*len),
                    true,
                )?;
            }
        }

        // The young part goes last, in entries of its own, so that a later
        // transaction can reclaim it apart from the rest by their keys.
        let young = txn.space.young_runs();
        let mut group: Vec<Vec<u8>> = txn
            .space
            .retired_runs()
            .chunks(runs_per_value(page_size))
            .map(encode_runs)
            .collect();
        let young_first = group.len();

        group.extend(young.chunks(runs_per_value(page_size)).map(encode_runs));

        let young_entries = group.len() - young_first;

        if group != written_group {
            for (sequence, value) in group.iter().enumerate() {
                btree::insert(
                    &loader,
                    &mut txn.space,
                    RETAINED_TREE,
                    &mut txn.retained_root,
                    &retained_key(txn.txn, sequence_number(sequence)?),
                    value,
                    true,
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
            if !young.is_empty() {
                txn.young_part = Some(YoungPart {
                    first: sequence_number(young_first)?,
                    entries: sequence_number(young_entries)?,
                    runs: young,
                });
            }

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
    let pager = Arc::clone(&txn.shared.pager);

    match take(txn) {
        Some(root) => btree::finish(&pager, txn.txn, tree, root, pages),
        None => Ok(Pointer::NULL),
    }
}

/// Writes the pages and the record, issues the barrier if the commit is to
/// be durable now, and publishes.
fn write_and_publish(
    txn: &mut WriteTransaction,
    mut pages: Vec<FinishedPage>,
    record: &CommitRecord,
    durability: Durability,
) -> Result<()> {
    let shared = Arc::clone(&txn.shared);
    let pager = &shared.pager;
    let written = pages.len() as u64;
    // A deferred commit that would take the window past its limits is made
    // durable instead.
    let deferred = durability == Durability::Deferred && shared.may_defer(written);

    pages.sort_unstable_by_key(|page| page.page);

    // Consecutive pages go out together, a run at a time.
    for run in pages
        .chunk_by(|before, after| after.page == before.page + 1)
        .flat_map(|run| run.chunks(pager.run_pages()))
    {
        match run {
            [page] => pager.write_sealed_run(page.page, &page.bytes)?,
            _ => {
                let bytes: Vec<&[u8]> = run.iter().map(|page| page.bytes.as_slice()).collect();

                pager.write_sealed_run(run[0].page, &bytes.concat())?;
            }
        }
    }

    // The file has to hold every page the record counts, including pages at
    // the end that were allocated and released again without being written.
    // Recovery skips a record that counts more pages than the file holds.
    let needed = record.page_count * pager.page_size() as u64;

    if pager.file_len()? < needed {
        pager.resize(record.page_count)?;
    }

    if txn.barrier_first {
        shared.barrier()?;
    }

    pager.write_header(&record.encode(txn.slot), slot_offset(txn.slot))?;

    // The commit point of a sync commit. From here on it survives a power cut,
    // and so does every deferred commit before it.
    if !deferred {
        shared.barrier()?;
    }

    let selector = Selector {
        slot: txn.slot,
        unsynced: deferred,
    };

    if let Err(error) = pager.write_header(&[selector.encode()], SELECTOR_OFFSET) {
        // After a barrier, the commit is durable, and recovery will publish it
        // when the file is opened again; until then this process cannot tell
        // other handles. Either way the file has to be opened again.
        shared.fail_sync();

        return Err(error);
    }

    if deferred {
        shared.extend_window(written);
    } else {
        shared.close_window();

        // A deferred commit never shortens the file: a power cut can take the
        // file back to the durable commit, which may count more pages.
        if pager.file_len()? > needed {
            // Only free pages lie past the new end. Failing to cut them off
            // costs space, nothing else, so the commit does not fail over it.
            let _ = pager.resize(record.page_count);
        }
    }

    let mut records = txn.header.records;

    records[txn.slot] = Some(*record);
    shared.set_header(Header { selector, records });
    shared.leave_free_runs(record.txn, txn.space.take_free());

    let mut young = txn.young.take().unwrap_or_default();

    if deferred {
        for group in &txn.young_reclaimed {
            young.remove(group);
        }

        if let Some(part) = txn.young_part.take() {
            young.insert(record.txn, part);
        }
    } else {
        // Every group is the new durable commit's or older, and is reclaimed
        // whole once no snapshot needs it.
        young.clear();
    }

    shared.leave_young(record.txn, young);

    for page in pages {
        shared
            .cache
            .insert(page.page, page.pointer.check, Arc::new(page.loaded));
    }

    Ok(())
}
