//! Write transactions: everything up to the commit.

use std::collections::BTreeMap;
use std::ops::{Bound, RangeBounds};
use std::sync::Arc;

use super::{Range, catalog_names, check_key, check_value, find_tree, root_child, tree_key};
use crate::btree::{self, Child, Load};
use crate::error::{Error, Result};
use crate::format::{
    CommitRecord, FREE_TREE, KEY_BLOCK_LEN, RETAINED_TREE, SLOT_COUNT, Selector, TXN_LIMIT,
    decode_free_key, decode_free_value, decode_retained_key, decode_runs,
};
use crate::instance::{Header, Shared, WriterGuard};
use crate::space::Space;

/// A tree this transaction has opened.
#[derive(Debug)]
pub(super) struct TreeState {
    pub(super) id: u64,
    pub(super) root: Option<Child>,
    pub(super) entries: u64,
    /// Whether the catalog held the tree when the transaction started.
    pub(super) existed: bool,
    /// Whether the tree was deleted and not created again.
    pub(super) deleted: bool,
    /// Whether anything about the tree changed.
    pub(super) changed: bool,
}

/// Changes to the database that become visible together, when
/// [`commit`](WriteTransaction::commit) returns, or not at all.
///
/// There is one write transaction at a time per file. Dropping one without
/// committing aborts it; nothing it did reaches the file's committed state.
/// If an operation fails, the transaction can no longer commit.
#[derive(Debug)]
pub struct WriteTransaction {
    pub(super) shared: Arc<Shared>,
    pub(super) _writer: WriterGuard,
    pub(super) header: Header,
    /// The durable commit: its slot is never written, its pages never reused.
    pub(super) durable: CommitRecord,
    /// The slot the new record goes into.
    pub(super) slot: usize,
    /// Whether the commit has to issue a barrier before it writes its record,
    /// because the slot may be one a power cut would make recovery trust.
    pub(super) barrier_first: bool,
    pub(super) txn: u64,
    pub(super) space: Space,
    pub(super) catalog: Option<Child>,
    pub(super) free_root: Option<Child>,
    pub(super) retained_root: Option<Child>,
    /// The transaction id of the published commit it started from.
    pub(super) base_txn: u64,
    /// The key block the commit record gets: the base commit's, unless the
    /// key is being changed.
    pub(super) key_block: [u8; KEY_BLOCK_LEN],
    /// The retained groups this transaction reclaims, by key.
    pub(super) reclaimed: Vec<Vec<u8>>,
    pub(super) trees: BTreeMap<String, TreeState>,
    pub(super) next_tree_id: u64,
    pub(super) failed: bool,
}

impl WriteTransaction {
    pub(crate) fn begin(shared: &Arc<Shared>) -> Result<Self> {
        let writer = shared.acquire_writer()?;

        shared.release_idle_snapshots();

        let header = shared.refresh_header()?;
        let base = header.published()?;
        let durable = if header.selector.unsynced {
            header
                .records
                .iter()
                .flatten()
                .find(|record| record.txn == base.durable_txn)
                .copied()
                .ok_or_else(|| {
                    shared
                        .pager
                        .corrupted("the durable commit's record is missing".to_owned())
                })?
        } else {
            base
        };
        let txn = 1 + header
            .records
            .iter()
            .flatten()
            .map(|record| record.txn)
            .max()
            .unwrap_or(0);

        // Only a damaged or forged file gets here: it takes 2^62 commits.
        if txn >= TXN_LIMIT {
            return Err(shared
                .pager
                .corrupted("the file has used every transaction id it may".to_owned()));
        }

        let (slot, barrier_first) = choose_slot(&header, &base, &durable, shared.last_barrier());
        let loader = &shared.loader;
        let free_root = root_child(base.free);
        let retained_root = root_child(base.retained);
        let free = match shared.take_free_runs(base.txn) {
            Some(free) => {
                // The crash suite runs through here thousands of times.
                #[cfg(test)]
                assert_eq!(
                    free,
                    load_free(loader, free_root.as_ref(), base.page_count)?,
                    "the free runs left by the last transaction differ from the free tree"
                );

                free
            }
            None => load_free(loader, free_root.as_ref(), base.page_count)?,
        };
        let mut space = Space::new(Arc::clone(&shared.pager), txn, base.page_count, free);

        // Reclaim every retained group that no snapshot and no possible
        // recovery can still reach. Recovery can go back to the durable
        // commit, which reaches every group above it.
        let mut retained = Vec::new();

        for entry in btree::Range::new(
            loader,
            RETAINED_TREE,
            retained_root.as_ref(),
            Bound::Unbounded,
            Bound::Unbounded,
        )? {
            let (key, value) = entry?;
            let (group, _) =
                decode_retained_key(&key).map_err(|reason| corrupted(shared, reason))?;

            if group > durable.txn {
                break;
            }

            retained.push((group, key, value));
        }

        let mut groups: Vec<u64> = retained.iter().map(|(group, _, _)| *group).collect();

        groups.dedup();

        let limit = shared.reclaimable(&groups)?;
        let mut reclaimed = Vec::new();

        for (group, key, value) in retained {
            if limit.is_none_or(|limit| group > limit) {
                break;
            }

            for (start, len) in decode_runs(&value).map_err(|reason| corrupted(shared, reason))? {
                space
                    .add_free_checked(start, u64::from(len))
                    .map_err(|reason| corrupted(shared, reason))?;
            }

            reclaimed.push(key);
        }

        Ok(Self {
            shared: Arc::clone(shared),
            _writer: writer,
            header,
            durable,
            slot,
            barrier_first,
            txn,
            space,
            catalog: root_child(base.catalog),
            free_root,
            retained_root,
            base_txn: base.txn,
            key_block: base.key_block,
            reclaimed,
            trees: BTreeMap::new(),
            next_tree_id: base.next_tree_id,
            failed: false,
        })
    }

    /// Stores `value` under `key` in tree `tree`, replacing any value already
    /// there. The tree is created if it does not exist.
    ///
    /// A key is at most a quarter of a page long, less a few bytes: 957 bytes
    /// with the default page size. A value is less than 4 GiB long; a value
    /// too large to keep in the tree's pages is stored in pages of its own.
    pub fn insert(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_open()?;

        let result = self.insert_inner(tree, key, value);

        self.failed |= result.is_err();

        result
    }

    fn insert_inner(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let loader = &self.shared.loader;

        check_key(key, loader.page_size())?;
        check_value(value)?;

        let state = open_tree(
            loader,
            self.catalog.as_ref(),
            &mut self.trees,
            &mut self.next_tree_id,
            tree,
            true,
        )?
        .ok_or_else(|| Error::Internal {
            message: "a tree was not created".to_owned(),
        })?;
        let replaced = btree::insert(
            loader,
            &mut self.space,
            state.id,
            &mut state.root,
            key,
            value,
        )?;

        if !replaced {
            state.entries += 1;
        }

        state.changed = true;

        Ok(())
    }

    /// Removes `key` and its value from tree `tree`. Returns whether it was
    /// there.
    pub fn remove(&mut self, tree: &str, key: &[u8]) -> Result<bool> {
        self.check_open()?;

        let result = self.remove_inner(tree, key);

        self.failed |= result.is_err();

        result
    }

    fn remove_inner(&mut self, tree: &str, key: &[u8]) -> Result<bool> {
        let loader = &self.shared.loader;

        check_key(key, loader.page_size())?;

        let Some(state) = open_tree(
            loader,
            self.catalog.as_ref(),
            &mut self.trees,
            &mut self.next_tree_id,
            tree,
            false,
        )?
        else {
            return Ok(false);
        };

        if !btree::remove(loader, &mut self.space, state.id, &mut state.root, key)? {
            return Ok(false);
        }

        state.entries -= 1;
        state.changed = true;

        Ok(true)
    }

    /// Deletes tree `tree` with everything in it. Returns whether it existed.
    pub fn delete_tree(&mut self, tree: &str) -> Result<bool> {
        self.check_open()?;

        let result = self.delete_tree_inner(tree);

        self.failed |= result.is_err();

        result
    }

    fn delete_tree_inner(&mut self, tree: &str) -> Result<bool> {
        let loader = &self.shared.loader;
        let Some(state) = open_tree(
            loader,
            self.catalog.as_ref(),
            &mut self.trees,
            &mut self.next_tree_id,
            tree,
            false,
        )?
        else {
            return Ok(false);
        };

        btree::delete_tree(loader, &mut self.space, state.id, state.root.take())?;
        state.entries = 0;
        state.deleted = true;
        state.changed = true;

        Ok(true)
    }

    /// The value stored under `key` in tree `tree`, including changes made in
    /// this transaction.
    pub fn get(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.check_open()?;

        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;

        match self.trees.get(tree) {
            Some(state) if state.deleted => Ok(None),
            Some(state) => btree::get(loader, state.id, state.root.as_ref(), key),
            None => match find_tree(loader, self.catalog.as_ref(), name)? {
                Some(descriptor) => btree::get(
                    loader,
                    descriptor.id,
                    root_child(descriptor.root).as_ref(),
                    key,
                ),
                None => Ok(None),
            },
        }
    }

    /// Every entry of tree `tree`, in key order, including changes made in
    /// this transaction.
    pub fn iter(&self, tree: &str) -> Result<Range<'_>> {
        self.range::<&[u8]>(tree, ..)
    }

    /// The entries of tree `tree` whose keys lie within `range`, in key
    /// order, including changes made in this transaction.
    pub fn range<K: AsRef<[u8]>>(
        &self,
        tree: &str,
        range: impl RangeBounds<K>,
    ) -> Result<Range<'_>> {
        self.check_open()?;

        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;

        match self.trees.get(tree) {
            Some(state) if state.deleted => Ok(Range::empty()),
            Some(state) => Range::over(loader, state.id, state.root.as_ref(), &range),
            None => match find_tree(loader, self.catalog.as_ref(), name)? {
                Some(descriptor) if !descriptor.root.is_null() => {
                    Range::over_committed(loader, descriptor.id, descriptor.root, &range)
                }
                _ => Ok(Range::empty()),
            },
        }
    }

    /// The number of entries in tree `tree`, including changes made in this
    /// transaction.
    pub fn len(&self, tree: &str) -> Result<u64> {
        self.check_open()?;

        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;

        match self.trees.get(tree) {
            Some(state) => Ok(state.entries),
            None => Ok(find_tree(loader, self.catalog.as_ref(), name)?
                .map_or(0, |descriptor| descriptor.entries)),
        }
    }

    /// The names of every tree, including trees created and deleted in this
    /// transaction, in byte order.
    pub fn tree_names(&self) -> Result<Vec<String>> {
        self.check_open()?;

        let mut names = catalog_names(&self.shared.loader, self.catalog.as_ref())?;

        for (name, state) in &self.trees {
            if state.deleted {
                names.retain(|existing| existing != name);
            } else if !state.existed && !names.contains(name) {
                names.push(name.clone());
            }
        }

        names.sort_unstable();

        Ok(names)
    }

    /// Makes every change of this transaction visible and durable, together.
    ///
    /// When it returns, the changes survive a crash or a power cut. If it
    /// fails with `SYNC_FAILED`, the outcome is unknown and the database has
    /// to be opened again.
    pub fn commit(self) -> Result<()> {
        if self.failed {
            return Err(Error::InvalidArgument {
                message: "an operation in this write transaction failed, so it can only be aborted"
                    .to_owned(),
            });
        }

        super::commit::commit(self, super::commit::Durability::Sync)
    }

    /// Makes every change of this transaction visible together, without
    /// waiting for it to be durable.
    ///
    /// Readers see the changes as soon as it returns. They become durable at
    /// the next barrier: the next [`commit`](Self::commit), a call to
    /// [`Database::sync`](crate::Database::sync), closing the database, or the
    /// engine's own limits on how much may wait (see
    /// [`OpenOptions::max_unsynced_pages`](crate::OpenOptions::max_unsynced_pages)).
    /// A crash of the process loses none of them. A power cut may undo deferred
    /// commits, newest first and never leaving a gap, and never damages the
    /// file.
    pub fn commit_deferred(self) -> Result<()> {
        if self.failed {
            return Err(Error::InvalidArgument {
                message: "an operation in this write transaction failed, so it can only be aborted"
                    .to_owned(),
            });
        }

        super::commit::commit(self, super::commit::Durability::Deferred)
    }

    /// Throws away every change of this transaction. The same as dropping it.
    pub fn abort(self) {}

    /// Makes the commit write `block` as its key block, to change the key.
    pub(crate) fn replace_key_block(&mut self, block: [u8; KEY_BLOCK_LEN]) {
        self.key_block = block;
    }

    fn check_open(&self) -> Result<()> {
        self.shared.check_usable()
    }
}

impl Drop for WriteTransaction {
    /// Leaves the free runs the transaction started from to the next one,
    /// unless it committed, which leaves its own.
    fn drop(&mut self) {
        if let Some(free) = self.space.take_initial_free() {
            self.shared.leave_free_runs(self.base_txn, free);
        }
    }
}

/// The state of tree `name` in this transaction, loaded from the catalog on
/// first use. With `create`, a tree that does not exist is created.
fn open_tree<'t>(
    loader: &crate::btree::Loader,
    catalog: Option<&Child>,
    trees: &'t mut BTreeMap<String, TreeState>,
    next_tree_id: &mut u64,
    name: &str,
    create: bool,
) -> Result<Option<&'t mut TreeState>> {
    let key = tree_key(name, loader.page_size())?;

    if !trees.contains_key(name) {
        let state = match find_tree(loader, catalog, key)? {
            Some(descriptor) => TreeState {
                id: descriptor.id,
                root: root_child(descriptor.root),
                entries: descriptor.entries,
                existed: true,
                deleted: false,
                changed: false,
            },
            None if create => {
                let id = *next_tree_id;

                *next_tree_id += 1;

                TreeState {
                    id,
                    root: None,
                    entries: 0,
                    existed: false,
                    deleted: false,
                    changed: true,
                }
            }
            None => return Ok(None),
        };

        trees.insert(name.to_owned(), state);
    }

    let Some(state) = trees.get_mut(name) else {
        return Ok(None);
    };

    if state.deleted {
        if !create {
            return Ok(None);
        }

        // A tree created again after being deleted is a new tree, with an id
        // of its own: an id is never reused.
        state.id = *next_tree_id;
        *next_tree_id += 1;
        state.deleted = false;
    }

    Ok(Some(state))
}

/// The slot for the new record, and whether a barrier has to come before it.
///
/// The slot holds neither the published commit nor the durable commit, nor
/// the commit that `last_barrier`, the selector a power cut can bring back,
/// names with its unsynced bit clear: recovery would trust that record
/// without checking it. When no slot qualifies, or `last_barrier` is unknown,
/// the commit issues a barrier first, which makes the published commit's
/// selector the one a power cut brings back. Of two candidates, one holding a
/// record newer than the published commit, which a writer that died before
/// publishing left behind, goes first; otherwise the older.
fn choose_slot(
    header: &Header,
    base: &CommitRecord,
    durable: &CommitRecord,
    last_barrier: Option<Selector>,
) -> (usize, bool) {
    let mut candidates: Vec<usize> = (0..SLOT_COUNT)
        .filter(|slot| *slot != header.selector.slot)
        .filter(|slot| header.records[*slot].is_none_or(|record| record.txn != durable.txn))
        .collect();

    candidates.sort_by_key(|slot| {
        let txn = header.records[*slot].map_or(0, |record| record.txn);

        // Newer than the published commit sorts first, then oldest first.
        (txn <= base.txn, txn)
    });

    let trusted = match last_barrier {
        Some(selector) if selector.unsynced => None,
        Some(selector) => Some(selector.slot),
        None => return (candidates[0], true),
    };

    match candidates.iter().find(|slot| Some(**slot) != trusted) {
        Some(slot) => (*slot, false),
        None => (candidates[0], true),
    }
}

/// The free runs of the free tree rooted at `root`.
fn load_free(
    loader: &crate::btree::Loader,
    root: Option<&Child>,
    page_count: u64,
) -> Result<BTreeMap<u64, u64>> {
    let mut free = BTreeMap::new();
    let mut end = 1;

    for entry in btree::Range::new(loader, FREE_TREE, root, Bound::Unbounded, Bound::Unbounded)? {
        let (key, value) = entry?;
        let start =
            decode_free_key(&key).map_err(|reason| loader.corrupted_file(reason.to_owned()))?;
        let len =
            decode_free_value(&value).map_err(|reason| loader.corrupted_file(reason.to_owned()))?;

        if start < end || len == 0 || start.saturating_add(len) > page_count {
            return Err(loader.corrupted_file("the free tree holds an impossible run".to_owned()));
        }

        end = start + len;
        free.insert(start, len);
    }

    Ok(free)
}

fn corrupted(shared: &Shared, reason: &str) -> Error {
    shared
        .pager
        .corrupted(format!("the retained tree: {reason}"))
}
