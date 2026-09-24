//! Opening a file after a crash: choosing the commit to adopt.
//!
//! `design/commits-and-recovery.md` is the specification. In short: go
//! through the valid commit records from the newest down, and adopt the first
//! that is either the published commit with the unsynced bit clear, or passes
//! [`check_commit`]. The durable commit always qualifies, so recovery never
//! goes back past the last commit that was reported durable.

use crate::btree::{Load, Loader, Node};
use crate::crypto::RecordAuth;
use crate::error::{Error, Result};
use crate::format::{
    CATALOG_TREE, CommitRecord, FREE_TREE, HEADER_LEN, Pointer, RETAINED_TREE, SELECTOR_OFFSET,
    SLOT_COUNT, Selector, StoredValue, TreeDescriptor, slot_offset,
};
use crate::instance::Header;
use crate::storage::Pager;

/// Reads the selector and the three records, adopts the commit recovery
/// chooses, and makes the file say so.
///
/// In an encrypted file, `auth` and the file id verify each record's MAC, and
/// a record that fails it is no candidate: no key holder wrote it.
///
/// Also returns the selector a power cut can bring back, when recovery wrote
/// it and issued a barrier after it; otherwise nobody knows which one it is.
pub(crate) fn recover(
    pager: &Pager,
    loader: &Loader,
    auth: Option<(&RecordAuth, &[u8; 16])>,
) -> Result<(Header, Option<Selector>)> {
    let bytes = pager.read_header(HEADER_LEN)?;
    let selector = Selector::decode(bytes[SELECTOR_OFFSET])
        .map_err(|reason| pager.corrupted(reason.to_owned()))?;
    let file_pages = pager.file_len()? / pager.page_size() as u64;
    let mut intact = [None; SLOT_COUNT];

    for (slot, record) in intact.iter_mut().enumerate() {
        // A record that fails its check is what a torn or lost write of a
        // record looks like.
        *record = CommitRecord::decode(slot, &bytes[slot_offset(slot)..])
            .ok()
            .flatten();
    }

    // Nor is a record that counts more pages than the file holds a candidate:
    // the writes that grew the file did not all survive.
    let mut records = intact.map(|record| record.filter(|record| record.page_count <= file_pages));

    for (slot, record) in records.iter_mut().enumerate() {
        let signed = |record: &CommitRecord| {
            auth.is_none_or(|(auth, file_id)| {
                auth.verify(file_id, slot, &record.authenticated(), &record.mac)
            })
        };

        *record = record.filter(signed);
    }

    let mut candidates: Vec<(usize, CommitRecord)> = records
        .iter()
        .enumerate()
        .filter_map(|(slot, record)| record.map(|record| (slot, record)))
        .collect();

    candidates.sort_by_key(|(_, record)| std::cmp::Reverse(record.txn));

    let mut adopted = None;

    for (slot, record) in candidates {
        let published_and_durable = slot == selector.slot && !selector.unsynced;

        if published_and_durable || check_commit(loader, &record)? {
            adopted = Some((slot, record));

            break;
        }
    }

    let Some((slot, record)) = adopted else {
        return Err(pager.corrupted("no commit record in the header can be used".to_owned()));
    };

    let adopted = Selector {
        slot,
        unsynced: false,
    };

    // Every other record at least as new as the adopted one goes: those newer
    // lost the race to the disk, and one the file is too short for now would
    // look whole again once the file grows.
    let stale: Vec<usize> = (0..SLOT_COUNT)
        .filter(|other| *other != slot)
        .filter(|other| intact[*other].is_some_and(|stale| stale.txn >= record.txn))
        .collect();
    let last_barrier = if slot != selector.slot || selector.unsynced || !stale.is_empty() {
        publish_recovered(pager, &stale, slot)?;

        for other in stale {
            records[other] = None;
        }

        Some(adopted)
    } else {
        None
    };

    if pager.file_len()? > record.page_count * pager.page_size() as u64 {
        pager.resize(record.page_count)?;
    }

    Ok((
        Header {
            selector: adopted,
            records,
        },
        last_barrier,
    ))
}

/// Makes the adopted commit the published and durable one: a barrier, the
/// stale records erased, the selector written, another barrier.
fn publish_recovered(pager: &Pager, stale: &[usize], slot: usize) -> Result<()> {
    let sync = |pager: &Pager| {
        pager.sync().map_err(|source| Error::SyncFailed {
            path: pager.path().to_path_buf(),
            source: Some(source),
        })
    };

    sync(pager)?;

    for other in stale {
        pager.write_header(&[0u8; crate::format::RECORD_LEN], slot_offset(*other))?;
    }

    let selector = Selector {
        slot,
        unsynced: false,
    };

    pager.write_header(&[selector.encode()], SELECTOR_OFFSET)?;
    sync(pager)
}

/// Whether every page `record` reaches and wrote after its durable
/// transaction made it to the disk intact.
///
/// Pages written at or before the durable transaction were durable before
/// the record was, and the record still reaches them, so nothing has
/// overwritten them: they are not read. The transaction id in every pointer is
/// what tells the two apart without reading the page.
pub(crate) fn check_commit(loader: &Loader, record: &CommitRecord) -> Result<bool> {
    let floor = record.durable_txn;
    let mut pending: Vec<(Pointer, u64, Option<u8>)> = [
        (record.catalog, CATALOG_TREE),
        (record.free, FREE_TREE),
        (record.retained, RETAINED_TREE),
    ]
    .into_iter()
    .filter(|(root, _)| !root.is_null() && root.txn > floor)
    .map(|(root, tree)| (root, tree, None))
    .collect();

    while let Some((pointer, tree, level)) = pending.pop() {
        if pointer.page >= record.page_count {
            return Ok(false);
        }

        let loaded = match loader.load(&pointer, tree, level) {
            Ok(loaded) => loaded,
            Err(Error::Corrupted { .. }) => return Ok(false),
            Err(error) => return Err(error),
        };

        match &loaded.to_node()? {
            Node::Branch(branch) => {
                for child in &branch.children {
                    if let crate::btree::Child::Clean(child) = child {
                        if child.txn > floor {
                            pending.push((*child, tree, Some(branch.level - 1)));
                        }
                    }
                }
            }
            Node::Leaf(entries) => {
                for entry in entries {
                    if let StoredValue::Overflow(reference) = &entry.value {
                        if reference.txn > floor {
                            if reference.first + u64::from(reference.pages) > record.page_count {
                                return Ok(false);
                            }

                            match loader.read_overflow(reference, tree) {
                                Ok(_) => {}
                                Err(Error::Corrupted { .. }) => return Ok(false),
                                Err(error) => return Err(error),
                            }
                        }
                    }

                    if tree == CATALOG_TREE {
                        let StoredValue::Inline(value) = &entry.value else {
                            return Ok(false);
                        };
                        let Ok(descriptor) = TreeDescriptor::decode(value) else {
                            return Ok(false);
                        };

                        if !descriptor.root.is_null() && descriptor.root.txn > floor {
                            pending.push((descriptor.root, descriptor.id, None));
                        }
                    }
                }
            }
        }
    }

    Ok(true)
}
