//! Write transactions: everything up to the commit.

use std::collections::BTreeMap;
use std::ops::{Bound, RangeBounds};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{
    Descriptors, Range, Seeker, catalog_names, check_key, check_value, engine_tree, find_tree,
    root_child, tree_key, user_tree,
};
use crate::btree::{self, Child, Load};
use crate::error::{Error, Result};
use crate::format::object::schema::OpenSchema;
use crate::format::{
    CATALOG_TREE, CommitRecord, FREE_TREE, KEY_BLOCK_LEN, RETAINED_TREE, SLOT_COUNT, Selector,
    TXN_LIMIT, decode_free_key, decode_free_value, decode_retained_key, decode_runs, max_key_len,
    retained_key,
};
use crate::instance::{Header, Shared, WriterGuard};
use crate::space::{Space, YoungPart, YoungParts};

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

/// The trees a write transaction has opened. Their states stay where they
/// were put, in the order they were opened, and the map of names leads to
/// each one's position, so that reaching an open tree goes down the map
/// once and leaves it. A map from names to the states themselves was gone
/// down twice for each change of an open tree, once to tell whether it was
/// open and once to reach its state, since a borrow of the state that the
/// caller keeps cannot come out of a lookup that may insert; that took
/// about a fortieth of an insert.
#[derive(Debug, Default)]
pub(super) struct Trees {
    states: Vec<TreeState>,
    positions: BTreeMap<String, usize>,
}

impl Trees {
    /// The state of tree `name`, if it is open.
    fn get(&self, name: &str) -> Option<&TreeState> {
        self.positions
            .get(name)
            .map(|&position| &self.states[position])
    }

    /// Every open tree, in byte order of the names.
    fn iter(&self) -> impl Iterator<Item = (&String, &TreeState)> {
        self.positions
            .iter()
            .map(|(name, &position)| (name, &self.states[position]))
    }

    /// Every open tree, taken out, in byte order of the names.
    pub(super) fn into_sorted(self) -> impl Iterator<Item = (String, TreeState)> {
        let mut states: Vec<_> = self.states.into_iter().map(Some).collect();

        self.positions
            .into_iter()
            .filter_map(move |(name, position)| Some((name, states[position].take()?)))
    }
}

/// Buffers the object layer lends itself between the objects a write
/// transaction stores, changes and deletes: an object's record, its key,
/// its index entries and those of the object it replaces, and where each
/// field lies in the record being stored (`placed`), in a binding's record
/// or changes (`given`), and in the record an update replaces
/// (`replaced`). A write takes the ones it needs and gives them back
/// cleared, so that a transaction writing many objects allocates them once
/// rather than for each object: allocating and freeing them took about a
/// tenth of an insert.
#[derive(Debug, Default)]
pub(crate) struct Spare {
    pub(crate) record: Vec<u8>,
    pub(crate) key: Vec<u8>,
    pub(crate) entries: EntryBuffers,
    pub(crate) old: EntryBuffers,
    pub(crate) placed: Vec<(usize, usize)>,
    pub(crate) given: Vec<(usize, usize)>,
    pub(crate) replaced: Vec<(usize, usize)>,
}

/// The buffers of an object's index entries in [`Spare`]: the keys of the
/// entries one after another, and the position of each one's index and
/// where its key ends.
#[derive(Debug, Default)]
pub(crate) struct EntryBuffers {
    pub(crate) bytes: Vec<u8>,
    pub(crate) ends: Vec<(usize, usize)>,
}

/// The most bytes a buffer [`kept`] keeps, so that one large object does
/// not hold its memory for the rest of the transaction.
const SPARE_MOST: usize = 64 * 1024;

/// `buffer` cleared, to give back to [`Spare`], or an empty one in its place
/// if it grew past [`SPARE_MOST`] bytes.
pub(crate) fn kept<T>(mut buffer: Vec<T>) -> Vec<T> {
    if buffer.capacity().saturating_mul(size_of::<T>()) > SPARE_MOST {
        return Vec::new();
    }

    buffer.clear();
    buffer
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
    /// The retained group entries this transaction reclaims, by key.
    pub(super) reclaimed: Vec<Vec<u8>>,
    /// The young parts of the base commit's retained groups, until the
    /// commit replaces them with its own. A transaction that does not commit
    /// leaves them for the next one.
    pub(super) young: Option<YoungParts>,
    /// The groups whose young parts this transaction reclaims.
    pub(super) young_reclaimed: Vec<u64>,
    /// The young part of this commit's retained group, once the commit has
    /// written the group.
    pub(super) young_part: Option<YoungPart>,
    pub(super) trees: Trees,
    /// The trees looked up in the catalog without being changed.
    pub(super) descriptors: Descriptors,
    /// Values waiting to be stored in the engine's trees before the commit,
    /// by tree and key ([`insert_later`](Self::insert_later)).
    pub(super) later: BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>,
    pub(super) next_tree_id: u64,
    pub(super) failed: bool,
    /// The schema of the handle that began the transaction, if it declared
    /// one.
    pub(super) schema: Option<Arc<OpenSchema>>,
    /// Whether the file holds `schema`, once a collection has checked. Only
    /// a migration writes the stored schema, and it sets the schema anew, so
    /// the answer holds for the rest of the transaction.
    pub(super) schema_checked: AtomicBool,
    /// The object layer's buffers, between the objects it stores.
    pub(super) spare: Spare,
}

impl WriteTransaction {
    pub(crate) fn begin(shared: &Arc<Shared>, schema: Option<Arc<OpenSchema>>) -> Result<Self> {
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
        let mut space = Space::new(
            Arc::clone(&shared.pager),
            txn,
            durable.txn,
            base.page_count,
            free,
            shared.cells(),
        );

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

        // Reclaim the young parts of the groups above the durable commit that
        // no snapshot can reach: pages written after the durable commit,
        // which only commits of the unsynced window reach, and recovery only
        // through a commit it checks. Groups the durable commit has caught up
        // with were reclaimed whole above, or wait to be.
        let mut young = shared.take_young(base.txn).unwrap_or_default();

        young.retain(|group, _| *group > durable.txn);

        // The crash suite runs through here thousands of times.
        #[cfg(test)]
        check_young(loader, retained_root.as_ref(), &young)?;

        let groups: Vec<u64> = young.keys().copied().collect();
        let mut young_reclaimed = Vec::new();

        if let Some(limit) = shared.young_reclaimable(durable.txn, &groups)? {
            for (group, part) in young.range(..=limit) {
                for (start, len) in &part.runs {
                    space
                        .add_free_checked(*start, u64::from(*len))
                        .map_err(|reason| corrupted(shared, reason))?;
                }

                for sequence in part.first..part.first + part.entries {
                    reclaimed.push(retained_key(*group, sequence).to_vec());
                }

                young_reclaimed.push(*group);
            }
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
            young: Some(young),
            young_reclaimed,
            young_part: None,
            trees: Trees::default(),
            descriptors: Descriptors::default(),
            later: BTreeMap::new(),
            next_tree_id: base.next_tree_id,
            failed: false,
            schema,
            schema_checked: AtomicBool::new(false),
            spare: Spare::default(),
        })
    }

    /// The schema of the handle that began the transaction.
    pub(crate) fn schema(&self) -> Option<&Arc<OpenSchema>> {
        self.schema.as_ref()
    }

    /// Makes the transaction's collections those of `schema`, which a
    /// migration is storing in it.
    pub(crate) fn set_schema(&mut self, schema: Arc<OpenSchema>) {
        self.schema = Some(schema);
        self.schema_checked = AtomicBool::new(false);
    }

    /// Whether a collection has found the file to hold the transaction's
    /// schema already.
    pub(crate) fn schema_checked(&self) -> &AtomicBool {
        &self.schema_checked
    }

    /// The object layer's buffers, to take from and give back to.
    pub(crate) fn spare(&mut self) -> &mut Spare {
        &mut self.spare
    }

    /// The longest key a tree of this file holds.
    pub(crate) fn max_key_len(&self) -> usize {
        max_key_len(self.shared.loader.page_size())
    }

    /// The error for damage found in the file.
    pub(crate) fn corrupted(&self, reason: String) -> Error {
        self.shared.loader.corrupted_file(reason)
    }

    /// Stores `value` under `key` in tree `tree`, replacing any value already
    /// there. The tree is created if it does not exist.
    ///
    /// A key is at most a quarter of a page long, less a few bytes: 957 bytes
    /// with the default page size. A value is less than 4 GiB long; a value
    /// too large to keep in the tree's pages is stored in pages of its own.
    pub fn insert(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()> {
        user_tree(tree)?;
        self.insert_in(tree, key, value)
    }

    /// [`insert`](Self::insert) into any tree, the engine's own included.
    pub(crate) fn insert_in(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_open()?;
        self.forget_later(tree, key);

        let result = self.insert_inner(tree, key, value, true, None).map(drop);

        self.failed |= result.is_err();

        result
    }

    /// Stores `value` under `key` in tree `tree` of the engine's, if nothing
    /// is stored there; returns whether it stored it. When it does not, the
    /// tree holds what it held, though the pages on the way to the key may
    /// have been copied. It saves the search a look-up before an insert
    /// would repeat.
    // Inlined by hand, as `btree::write::store_value` is, and for the same
    // reason.
    #[inline(always)]
    pub(crate) fn insert_new_in(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<bool> {
        self.check_open()?;
        self.store_later(tree)?;

        let result = self.insert_inner(tree, key, value, false, None);

        self.failed |= result.is_err();

        result
    }

    /// [`insert_in`](Self::insert_in), giving `visit` the value it replaces
    /// before it goes, for a caller that needs that value: the key is gone
    /// down to once, rather than once to read the value and once to replace
    /// it. Returns whether there was a value. An error `visit` returns
    /// stores nothing and leaves the transaction able to commit, though the
    /// pages on the way to the key may have been copied.
    pub(crate) fn insert_in_with(
        &mut self,
        tree: &str,
        key: &[u8],
        value: &[u8],
        visit: &mut btree::Removed<'_>,
    ) -> Result<bool> {
        self.check_open()?;

        // A value waiting to be stored is the key's, and any in the tree an
        // older one.
        if let Some(old) = self.later.get(tree).and_then(|waiting| waiting.get(key)) {
            visit(old)?;
            self.insert_in(tree, key, value)?;

            return Ok(true);
        }

        let mut refused = false;
        let result = self.insert_inner(
            tree,
            key,
            value,
            true,
            Some(&mut |old| visit(old).inspect_err(|_| refused = true)),
        );

        self.failed |= result.is_err() && !refused;

        result.map(|new| !new)
    }

    /// Replaces the value under `key` in tree `tree` of the engine's with
    /// what `change` makes of it, for a caller whose new value depends on the
    /// old: the key is gone down to once, rather than once to read the value
    /// and once to replace it. `change` writes the new value into `value`, a
    /// buffer the caller lends, and keeps the old one by returning false.
    /// Returns whether there was a value. An error `change` returns stores
    /// nothing and leaves the transaction able to commit, though the pages on
    /// the way to the key may have been copied, as they may be when the key
    /// is not there or the value is kept.
    pub(crate) fn update_in_with(
        &mut self,
        tree: &str,
        key: &[u8],
        value: &mut Vec<u8>,
        change: &mut btree::Change<'_>,
    ) -> Result<bool> {
        self.check_open()?;

        // A value waiting to be stored is the key's, and any in the tree an
        // older one.
        if let Some(old) = self
            .later
            .get(tree)
            .and_then(|waiting| waiting.get(key))
            .cloned()
        {
            if change(&old, value)? {
                check_value(value)?;
                self.insert_in(tree, key, value)?;
            }

            return Ok(true);
        }

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
        let mut refused = false;
        let result = btree::update_with(
            loader,
            &mut self.space,
            state.id,
            &mut state.root,
            key,
            value,
            &mut |old, value| {
                let changed = change(old, value).and_then(|changed| {
                    if changed {
                        check_value(value)?;
                    }

                    Ok(changed)
                });

                refused = changed.is_err();

                changed
            },
        );

        // The nodes on the way are copied whatever the change did: the
        // copies are the tree now, and the commit has to write them.
        state.changed = true;
        self.failed |= result.is_err() && !refused;

        result
    }

    /// Writes tree `name` again with its entries in as few pages as they fit
    /// in, when that saves a tenth of its pages or more, or with `always`
    /// whatever it saves (`btree::repack`), and returns whether it did. A
    /// tree this transaction has changed is left as it is.
    pub(crate) fn repack_tree(&mut self, name: &str, always: bool) -> Result<bool> {
        self.check_open()?;

        let result = self.repack_tree_inner(name, always);

        self.failed |= result.is_err();

        result
    }

    fn repack_tree_inner(&mut self, name: &str, always: bool) -> Result<bool> {
        let loader = self.shared.loader.clone();
        let Some(state) = open_tree(
            &loader,
            self.catalog.as_ref(),
            &mut self.trees,
            &mut self.next_tree_id,
            name,
            false,
        )?
        else {
            return Ok(false);
        };
        let Some(Child::Clean(root)) = &state.root else {
            return Ok(false);
        };

        let cells = btree::Store::cells(&self.space);

        if !always
            && !btree::occupancy(&loader, state.id, root, cells)?
                .worth_repacking(loader.page_size())
        {
            return Ok(false);
        }

        btree::repack(&loader, &mut self.space, state.id, &mut state.root)?;
        state.changed = true;

        Ok(true)
    }

    /// Moves every page at or above page `threshold` that the commit this
    /// transaction makes would still use, of every tree, the engine's own and
    /// the allocator trees included, into the lowest free pages: the nodes,
    /// every node on the way to one, and every overflow run. The pages given
    /// up are retained as any page a transaction stops using is, and a later
    /// commit reclaims them and cuts the file's free tail off. Returns how
    /// many pages at or above `threshold` it gave up.
    pub(crate) fn relocate_above(&mut self, threshold: u64) -> Result<u64> {
        self.check_open()?;

        let result = self.relocate_trees(threshold);

        self.failed |= result.is_err();

        result
    }

    fn relocate_trees(&mut self, threshold: u64) -> Result<u64> {
        let loader = self.shared.loader.clone();
        let mut moved = 0;

        for name in catalog_names(&loader, self.catalog.as_ref())? {
            let state = open_tree(
                &loader,
                self.catalog.as_ref(),
                &mut self.trees,
                &mut self.next_tree_id,
                &name,
                false,
            )?;
            let Some(state) = state else {
                continue;
            };
            let relocated = btree::relocate(
                &loader,
                &mut self.space,
                state.id,
                &mut state.root,
                threshold,
            )?;

            // A run with no room below the threshold leaves the nodes on
            // the way to it copied, with no page given up.
            state.changed |= relocated > 0 || matches!(state.root, Some(Child::Dirty { .. }));
            moved += relocated;
        }

        moved += btree::relocate(
            &loader,
            &mut self.space,
            CATALOG_TREE,
            &mut self.catalog,
            threshold,
        )?;
        moved += btree::relocate(
            &loader,
            &mut self.space,
            FREE_TREE,
            &mut self.free_root,
            threshold,
        )?;
        moved += btree::relocate(
            &loader,
            &mut self.space,
            RETAINED_TREE,
            &mut self.retained_root,
            threshold,
        )?;

        Ok(moved)
    }

    /// The pages the commit this transaction makes would count, and how many
    /// of them are free.
    pub(crate) fn space_summary(&self) -> (u64, u64) {
        (self.space.page_count(), self.space.free_pages())
    }

    /// Stores `value` under `key` in tree `tree` of the engine's when the
    /// transaction commits, and until then gives it to every read of the key.
    /// For a small value written many times in one transaction, such as a
    /// collection's auto-increment counter, which would otherwise cost a
    /// change to its tree every time. A tree holding such values may be read
    /// only by key, and changed; walking or counting it is refused.
    pub(crate) fn insert_later(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_open()?;
        check_key(key, self.shared.loader.page_size())?;
        check_value(value)?;

        // A value written again, as a counter is by every insert, replaces
        // the one waiting in its place, with no key or value copied anew.
        if let Some(waiting) = self
            .later
            .get_mut(tree)
            .and_then(|waiting| waiting.get_mut(key))
        {
            waiting.clear();
            waiting.extend_from_slice(value);

            return Ok(());
        }

        self.later
            .entry(tree.to_owned())
            .or_default()
            .insert(key.to_vec(), value.to_vec());

        Ok(())
    }

    /// Stores the values waiting for tree `tree`.
    fn store_later(&mut self, tree: &str) -> Result<()> {
        let Some(waiting) = self.later.remove(tree) else {
            return Ok(());
        };

        for (key, value) in waiting {
            let result = self.insert_inner(tree, &key, &value, true, None);

            self.failed |= result.is_err();
            result?;
        }

        Ok(())
    }

    /// Drops a value waiting for `key` of tree `tree`, which a change of the
    /// key replaces.
    fn forget_later(&mut self, tree: &str, key: &[u8]) -> bool {
        let Some(waiting) = self.later.get_mut(tree) else {
            return false;
        };
        let forgot = waiting.remove(key).is_some();

        if waiting.is_empty() {
            self.later.remove(tree);
        }

        forgot
    }

    /// Refuses to walk or count a tree with values waiting to be stored.
    fn check_not_waiting(&self, tree: &str) -> Result<()> {
        if self.later.contains_key(tree) {
            return Err(Error::Internal {
                message: format!("tree {tree:?} was walked with values waiting to be stored"),
            });
        }

        Ok(())
    }

    /// Inserts or, with `replace`, replaces; returns whether `key` was stored
    /// anew.
    fn insert_inner(
        &mut self,
        tree: &str,
        key: &[u8],
        value: &[u8],
        replace: bool,
        visit: Option<&mut btree::Removed<'_>>,
    ) -> Result<bool> {
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
        let outcome = match visit {
            Some(visit) => {
                // The nodes on the way are copied, and the committed ones
                // given back, whether the visitor lets the value be replaced
                // or not: the copies are the tree now, and the commit has to
                // write them.
                state.changed = true;
                btree::insert_with(
                    loader,
                    &mut self.space,
                    state.id,
                    &mut state.root,
                    key,
                    value,
                    visit,
                )?
            }
            None => btree::insert(
                loader,
                &mut self.space,
                state.id,
                &mut state.root,
                key,
                value,
                replace,
            )?,
        };

        if outcome == btree::Inserted::New {
            state.entries += 1;
        }

        state.changed = true;

        Ok(outcome == btree::Inserted::New)
    }

    /// Removes `key` and its value from tree `tree`. Returns whether it was
    /// there.
    pub fn remove(&mut self, tree: &str, key: &[u8]) -> Result<bool> {
        user_tree(tree)?;
        self.remove_in(tree, key)
    }

    /// [`remove`](Self::remove) from any tree, the engine's own included.
    pub(crate) fn remove_in(&mut self, tree: &str, key: &[u8]) -> Result<bool> {
        self.remove_either(tree, key, Removal::Checked)
    }

    /// [`remove_in`](Self::remove_in) for a key the caller knows is there,
    /// having just read it in this transaction: the removal goes down to it
    /// once rather than looking it up first, and copies the pages on the way
    /// even when it is not there after all.
    pub(crate) fn remove_present_in(&mut self, tree: &str, key: &[u8]) -> Result<bool> {
        self.remove_either(tree, key, Removal::Present)
    }

    /// [`remove_in`](Self::remove_in), giving `visit` the value before it
    /// goes, for a caller that needs the value of what it removes: the part
    /// of the way to the key that this transaction has changed already is
    /// gone down once, rather than once to read the value and once to remove
    /// it, and nothing is copied when the key is not there. An error `visit`
    /// returns removes nothing and leaves the transaction able to commit.
    pub(crate) fn remove_in_with(
        &mut self,
        tree: &str,
        key: &[u8],
        visit: &mut btree::Removed<'_>,
    ) -> Result<bool> {
        self.check_open()?;

        // A value waiting to be stored is the key's, and any in the tree an
        // older one.
        if let Some(value) = self.later.get(tree).and_then(|waiting| waiting.get(key)) {
            visit(value)?;

            return self.remove_either(tree, key, Removal::Checked);
        }

        let mut refused = false;
        let result = self.remove_inner(
            tree,
            key,
            Removal::Visited(&mut |value| visit(value).inspect_err(|_| refused = true)),
        );

        self.failed |= result.is_err() && !refused;

        result
    }

    fn remove_either(&mut self, tree: &str, key: &[u8], removal: Removal<'_>) -> Result<bool> {
        self.check_open()?;

        let waited = self.forget_later(tree, key);
        let result = self.remove_inner(tree, key, removal);

        self.failed |= result.is_err();

        Ok(result? || waited)
    }

    fn remove_inner(&mut self, tree: &str, key: &[u8], removal: Removal<'_>) -> Result<bool> {
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

        let removed = match removal {
            Removal::Checked => {
                btree::remove(loader, &mut self.space, state.id, &mut state.root, key)?
            }
            Removal::Present => {
                // The nodes on the way are copied, and the committed ones
                // given back, whether the key is there or not: the copies are
                // the tree now, and the commit has to write them.
                state.changed = true;
                btree::remove_present(loader, &mut self.space, state.id, &mut state.root, key)?
            }
            Removal::Visited(visit) => {
                let removed = btree::remove_with(
                    loader,
                    &mut self.space,
                    state.id,
                    &mut state.root,
                    key,
                    visit,
                );

                // The nodes on the way are copied only for a key that is
                // there, and stay copied when the visitor refuses it. A
                // copied root was copied by an earlier change or by this one.
                state.changed |= matches!(state.root, Some(Child::Dirty { .. }));
                removed?
            }
        };

        if !removed {
            return Ok(false);
        }

        state.entries -= 1;
        state.changed = true;

        Ok(true)
    }

    /// Deletes tree `tree` with everything in it. Returns whether it existed.
    pub fn delete_tree(&mut self, tree: &str) -> Result<bool> {
        user_tree(tree)?;
        self.delete_tree_in(tree)
    }

    /// [`delete_tree`](Self::delete_tree) for any tree, the engine's own
    /// included.
    pub(crate) fn delete_tree_in(&mut self, tree: &str) -> Result<bool> {
        self.check_open()?;
        self.later.remove(tree);

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
        user_tree(tree)?;
        self.get_in(tree, key)
    }

    /// [`get`](Self::get) in any tree, the engine's own included.
    pub(crate) fn get_in(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.check_open()?;

        if let Some(value) = self.later.get(tree).and_then(|waiting| waiting.get(key)) {
            return Ok(Some(value.clone()));
        }

        let loader = &self.shared.loader;

        match self.trees.get(tree) {
            Some(state) if state.deleted => Ok(None),
            Some(state) => btree::get(loader, state.id, state.root.as_ref(), key),
            None => match self.descriptors.find(
                loader,
                self.catalog.as_ref(),
                tree_key(tree, loader.page_size())?,
            )? {
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

    /// [`get_in`](Self::get_in), giving `visit` the value borrowed where it
    /// lies rather than copied, and returning whether there was one.
    pub(crate) fn get_in_with(
        &self,
        tree: &str,
        key: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        self.check_open()?;

        if let Some(value) = self.later.get(tree).and_then(|waiting| waiting.get(key)) {
            visit(value)?;

            return Ok(true);
        }

        let loader = &self.shared.loader;

        match self.trees.get(tree) {
            Some(state) if state.deleted => Ok(false),
            Some(state) => btree::get_with(loader, state.id, state.root.as_ref(), key, visit),
            None => match self.descriptors.find(
                loader,
                self.catalog.as_ref(),
                tree_key(tree, loader.page_size())?,
            )? {
                Some(descriptor) => btree::get_with(
                    loader,
                    descriptor.id,
                    root_child(descriptor.root).as_ref(),
                    key,
                    visit,
                ),
                None => Ok(false),
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
        user_tree(tree)?;
        self.range_in(tree, &range, false)
    }

    /// The entries of tree `tree` whose keys lie within `range`, in reverse
    /// key order, including changes made in this transaction.
    pub fn range_backward<K: AsRef<[u8]>>(
        &self,
        tree: &str,
        range: impl RangeBounds<K>,
    ) -> Result<Range<'_>> {
        user_tree(tree)?;
        self.range_in(tree, &range, true)
    }

    /// A range of any tree, the engine's own included, walked forwards or
    /// backwards.
    pub(crate) fn range_in<K: AsRef<[u8]>>(
        &self,
        tree: &str,
        range: &impl RangeBounds<K>,
        backward: bool,
    ) -> Result<Range<'_>> {
        self.check_open()?;
        self.check_not_waiting(tree)?;

        let loader = &self.shared.loader;

        match self.trees.get(tree) {
            Some(state) if state.deleted => Ok(Range::empty()),
            Some(state) => Range::over(loader, state.id, state.root.as_ref(), range, backward),
            None => match self.descriptors.find(
                loader,
                self.catalog.as_ref(),
                tree_key(tree, loader.page_size())?,
            )? {
                Some(descriptor) if !descriptor.root.is_null() => {
                    Range::over_committed(loader, descriptor.id, descriptor.root, range, backward)
                }
                _ => Ok(Range::empty()),
            },
        }
    }

    /// Lookups in tree `tree` of one key after another, each from where the
    /// last one ended, including changes made in this transaction; none find
    /// anything if the tree does not exist. Refused, as a walk is, for a tree
    /// with values waiting to be stored.
    pub(crate) fn seeker_in(&self, tree: &str) -> Result<Seeker<'_>> {
        self.check_open()?;
        self.check_not_waiting(tree)?;

        let loader = &self.shared.loader;

        match self.trees.get(tree) {
            Some(state) if state.deleted => Seeker::new(loader, state.id, None),
            Some(state) => Seeker::new(loader, state.id, state.root.as_ref()),
            None => match self.descriptors.find(
                loader,
                self.catalog.as_ref(),
                tree_key(tree, loader.page_size())?,
            )? {
                Some(descriptor) => Seeker::from_pointer(loader, descriptor.id, descriptor.root),
                None => Seeker::new(loader, 0, None),
            },
        }
    }

    /// The number of entries in tree `tree`, including changes made in this
    /// transaction.
    pub fn len(&self, tree: &str) -> Result<u64> {
        user_tree(tree)?;
        self.len_in(tree)
    }

    /// [`len`](Self::len) of any tree, the engine's own included.
    pub(crate) fn len_in(&self, tree: &str) -> Result<u64> {
        self.check_open()?;
        self.check_not_waiting(tree)?;

        let loader = &self.shared.loader;

        match self.trees.get(tree) {
            Some(state) => Ok(state.entries),
            None => Ok(self
                .descriptors
                .find(
                    loader,
                    self.catalog.as_ref(),
                    tree_key(tree, loader.page_size())?,
                )?
                .map_or(0, |descriptor| descriptor.entries)),
        }
    }

    /// The names of every tree, including trees created and deleted in this
    /// transaction, in byte order. The engine's own trees, which hold the
    /// objects of collections, are not among them.
    pub fn tree_names(&self) -> Result<Vec<String>> {
        self.check_open()?;

        let mut names = catalog_names(&self.shared.loader, self.catalog.as_ref())?;

        for (name, state) in self.trees.iter() {
            if state.deleted {
                names.retain(|existing| existing != name);
            } else if !state.existed && !names.contains(name) {
                names.push(name.clone());
            }
        }

        names.retain(|name| !engine_tree(name));
        names.sort_unstable();

        Ok(names)
    }

    /// Makes every change of this transaction visible and durable, together.
    ///
    /// When it returns, the changes survive a crash or a power cut. If it
    /// fails with `SYNC_FAILED`, the outcome is unknown and the database has
    /// to be opened again.
    pub fn commit(mut self) -> Result<()> {
        self.store_all_later()?;

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
    pub fn commit_deferred(mut self) -> Result<()> {
        self.store_all_later()?;

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

    /// Stores every value waiting to be stored, before the commit.
    fn store_all_later(&mut self) -> Result<()> {
        let trees: Vec<String> = self.later.keys().cloned().collect();

        for tree in trees {
            self.store_later(&tree)?;
        }

        Ok(())
    }

    /// Makes the commit write `block` as its key block, to change the key.
    pub(crate) fn replace_key_block(&mut self, block: [u8; KEY_BLOCK_LEN]) {
        self.key_block = block;
    }

    fn check_open(&self) -> Result<()> {
        self.shared.check_usable()
    }
}

impl Drop for WriteTransaction {
    /// Leaves the free runs and the young parts the transaction started from
    /// to the next one, unless it committed, which leaves its own.
    fn drop(&mut self) {
        if let Some(free) = self.space.take_initial_free() {
            self.shared.leave_free_runs(self.base_txn, free);
        }

        if let Some(young) = self.young.take() {
            self.shared.leave_young(self.base_txn, young);
        }
    }
}

/// How a removal finds its key.
enum Removal<'v> {
    /// Looked up first, and nothing copied when it is not there.
    Checked,
    /// Gone down to once, copying the nodes on the way whether it is there
    /// or not, for a key the caller knows is there.
    Present,
    /// Gone down to once, with its value given to the visitor before it
    /// goes, and nothing copied when it is not there.
    Visited(&'v mut btree::Removed<'v>),
}

/// The state of tree `name` in this transaction, loaded from the catalog on
/// first use, when its name is checked. With `create`, a tree that does not
/// exist is created.
fn open_tree<'t>(
    loader: &crate::btree::Loader,
    catalog: Option<&Child>,
    trees: &'t mut Trees,
    next_tree_id: &mut u64,
    name: &str,
    create: bool,
) -> Result<Option<&'t mut TreeState>> {
    let position = match trees.positions.get(name) {
        Some(&position) => position,
        None => {
            let key = tree_key(name, loader.page_size())?;
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

            trees.states.push(state);
            trees
                .positions
                .insert(name.to_owned(), trees.states.len() - 1);
            trees.states.len() - 1
        }
    };
    let state = &mut trees.states[position];

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
/// without checking it. When `last_barrier` is unknown, the newest record
/// older than the durable commit stands in for that commit; see
/// [`possibly_trusted`]. When no slot qualifies, the commit issues a barrier
/// first, which makes the published commit's selector the one a power cut
/// brings back. Of two candidates, one holding a record newer than the
/// published commit, which a writer that died before publishing left behind,
/// goes first; otherwise the older.
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
        None => possibly_trusted(header, durable),
    };

    match candidates.iter().find(|slot| Some(**slot) != trusted) {
        Some(slot) => (*slot, false),
        None => (candidates[0], true),
    }
}

/// The slot a power cut may make recovery trust, besides those of the
/// published and durable commits, for a writer that does not know the last
/// selector written before the last barrier: after opening the file, or
/// after another process has committed.
///
/// Every selector with the unsynced bit clear is written right after a
/// barrier, by a sync commit, by the end of an unsynced window or by
/// recovery, and names a commit that is durable from then on, so no record
/// newer than the durable commit was ever named by one. Of the older ones,
/// only the base of the durable commit can be: the selector written before
/// the barrier of the sync commit that made it durable. That writer and every
/// one after it kept the base's slot, as they kept whichever slot they knew a
/// power cut would bring back, or issued a barrier, after which the selector
/// before the last barrier names the published commit of that time. So the
/// base is still in its slot, and it is the newest record older than the
/// durable commit: a record a dead writer left between them would have been
/// the durable commit's writer's first choice of slot.
fn possibly_trusted(header: &Header, durable: &CommitRecord) -> Option<usize> {
    (0..SLOT_COUNT)
        .filter_map(|slot| header.records[slot].map(|record| (slot, record.txn)))
        .filter(|(_, txn)| *txn < durable.txn)
        .max_by_key(|(_, txn)| *txn)
        .map(|(slot, _)| slot)
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

/// Checks that the young parts left in memory are what the retained tree
/// rooted at `root` holds: each part's entries, and no entry after them.
#[cfg(test)]
fn check_young(
    loader: &crate::btree::Loader,
    root: Option<&Child>,
    young: &YoungParts,
) -> Result<()> {
    for (group, part) in young {
        let mut runs = Vec::new();
        let mut sequences = Vec::new();
        let first = retained_key(*group, part.first);
        let next_group = retained_key(*group + 1, 0);

        for entry in btree::Range::new(
            loader,
            RETAINED_TREE,
            root,
            Bound::Included(&first[..]),
            Bound::Excluded(&next_group[..]),
        )? {
            let (key, value) = entry?;

            sequences.push(decode_retained_key(&key).unwrap().1);
            runs.extend(decode_runs(&value).unwrap());
        }

        assert_eq!(
            sequences,
            (part.first..part.first + part.entries).collect::<Vec<_>>(),
            "the young part of group {group} left in memory names other entries"
        );
        assert_eq!(
            runs, part.runs,
            "the young part of group {group} left in memory differs from the retained tree"
        );
    }

    Ok(())
}

fn corrupted(shared: &Shared, reason: &str) -> Error {
    shared
        .pager
        .corrupted(format!("the retained tree: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{SPARE_MOST, kept};

    #[test]
    fn a_buffer_is_kept_cleared_unless_it_grew_past_the_limit() {
        let mut small = Vec::with_capacity(100);

        small.extend_from_slice(&[1u8; 50]);

        let small = kept(small);

        assert!(small.is_empty());
        assert!(small.capacity() >= 100);
        assert_eq!(kept(vec![0u8; SPARE_MOST + 1]).capacity(), 0);

        // The limit counts bytes, not elements.
        let pairs: Vec<(usize, usize)> = Vec::with_capacity(SPARE_MOST / 8);

        assert_eq!(kept(pairs).capacity(), 0);
    }
}
