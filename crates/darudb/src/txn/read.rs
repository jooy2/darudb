//! Read transactions.

use std::ops::RangeBounds;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use super::{
    Descriptors, Range, Seeker, catalog_names, engine_tree, root_child, tree_key, user_tree,
};
use crate::btree::{self, Load, LoadedNode};
use crate::error::{Error, Result};
use crate::format::CommitRecord;
use crate::format::object::schema::OpenSchema;
use crate::instance::Shared;

/// A consistent view of the database as of one commit.
///
/// Everything read through it comes from the same commit, whatever is
/// committed while it lives. Pages it can reach are not reused until it is
/// dropped, so a read transaction kept open for a long time makes the file
/// grow while other transactions write.
#[derive(Debug)]
pub struct ReadTransaction {
    shared: Arc<Shared>,
    record: CommitRecord,
    /// The schema of the handle that began the transaction, if it declared
    /// one.
    schema: Option<Arc<OpenSchema>>,
    /// Whether the commit this transaction sees holds `schema`, once a
    /// collection has checked. The commit does not change, so neither does
    /// the answer.
    schema_checked: AtomicBool,
    /// The trees looked up in the catalog so far.
    descriptors: Descriptors,
    /// The trees this transaction has looked keys up in, with their roots.
    roots: Mutex<Vec<KeptRoot>>,
}

/// A tree a read transaction has looked a key up in: its name, its id, and
/// its root node, `None` for an empty tree, or one that does not exist.
///
/// A lookup after the first finds the tree here, under a lock of the
/// transaction's own, and starts from its root without asking the page
/// cache: finding the tree under the lock that every read transaction of the
/// commit shares, and then its root in the page cache under the cache's
/// lock, took a tenth of a lookup by key on an x86 processor. The roots are
/// held until the transaction ends, so a writer in this process that changes
/// one of them in the meantime copies it rather than taking it from the
/// cache.
#[derive(Debug)]
struct KeptRoot {
    name: Box<[u8]>,
    id: u64,
    root: Option<Arc<LoadedNode>>,
}

/// How many trees a read transaction keeps the roots of; the few a query
/// reads fit.
const KEPT_ROOTS: usize = 16;

impl ReadTransaction {
    pub(crate) fn begin(shared: &Arc<Shared>, schema: Option<Arc<OpenSchema>>) -> Result<Self> {
        let record = shared.begin_snapshot()?;

        Ok(Self {
            shared: Arc::clone(shared),
            record,
            schema,
            schema_checked: AtomicBool::new(false),
            descriptors: Descriptors::shared(shared.learned(record.txn)),
            roots: Mutex::new(Vec::new()),
        })
    }

    /// The record of the commit this transaction sees.
    pub(crate) fn record(&self) -> &CommitRecord {
        &self.record
    }

    /// The schema of the handle that began the transaction.
    pub(crate) fn schema(&self) -> Option<&Arc<OpenSchema>> {
        self.schema.as_ref()
    }

    /// Whether a collection has found the file to hold the transaction's
    /// schema already.
    pub(crate) fn schema_checked(&self) -> &AtomicBool {
        &self.schema_checked
    }

    /// The error for damage found in the file.
    pub(crate) fn corrupted(&self, reason: String) -> Error {
        self.shared.loader.corrupted_file(reason)
    }

    /// The path the database was opened at, for an error that names the file.
    pub(crate) fn path(&self) -> &Path {
        &self.shared.path
    }

    /// The transaction id of the commit this transaction sees.
    ///
    /// It only grows from one commit to the next, so comparing it with an
    /// earlier value tells whether anything was committed in between.
    pub fn commit_id(&self) -> u64 {
        self.record.txn
    }

    /// The value stored under `key` in tree `tree`, if there is one. A tree
    /// that does not exist holds nothing.
    pub fn get(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        user_tree(tree)?;
        self.get_in(tree, key)
    }

    /// [`get`](Self::get) in any tree, the engine's own included.
    pub(crate) fn get_in(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        let Some(descriptor) = self.descriptors.find(loader, catalog.as_ref(), name)? else {
            return Ok(None);
        };

        btree::get(
            loader,
            descriptor.id,
            root_child(descriptor.root).as_ref(),
            key,
        )
    }

    /// [`get_in`](Self::get_in), for a value nearly every read transaction
    /// reads, such as the stored schema's record: kept for the other read
    /// transactions of this commit, which take it from there.
    pub(crate) fn get_in_kept(&self, tree: &str, key: &[u8]) -> Result<Option<Arc<[u8]>>> {
        if let Some(value) = self.descriptors.kept(tree, key) {
            return Ok(value);
        }

        let value: Option<Arc<[u8]>> = self.get_in(tree, key)?.map(Arc::from);

        self.descriptors.keep(tree, key, value.clone());

        Ok(value)
    }

    /// [`get_in`](Self::get_in), giving `visit` the value borrowed where it
    /// lies rather than copied, and returning whether there was one.
    pub(crate) fn get_in_with(
        &self,
        tree: &str,
        key: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;

        match self.root_of(name)? {
            Some((id, root)) => btree::get_with_root(loader, id, root, key, visit),
            None => Ok(false),
        }
    }

    /// The id and the root node of tree `name`, kept from the last lookup in
    /// it, or found and kept now; `None` for a tree that is empty or does not
    /// exist.
    fn root_of(&self, name: &[u8]) -> Result<Option<(u64, Arc<LoadedNode>)>> {
        let lock = || self.roots.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(kept) = lock().iter().find(|kept| *kept.name == *name) {
            return Ok(kept.root.clone().map(|root| (kept.id, root)));
        }

        let loader = &self.shared.loader;
        let catalog = root_child(self.record.catalog);
        let (id, root) = match self.descriptors.find(loader, catalog.as_ref(), name)? {
            Some(descriptor) if !descriptor.root.is_null() => (
                descriptor.id,
                Some(loader.load(&descriptor.root, descriptor.id, None)?),
            ),
            _ => (0, None),
        };
        let mut roots = lock();

        if roots.len() < KEPT_ROOTS && roots.iter().all(|kept| *kept.name != *name) {
            roots.push(KeptRoot {
                name: name.into(),
                id,
                root: root.clone(),
            });
        }

        Ok(root.map(|root| (id, root)))
    }

    /// Lookups in tree `tree` of one key after another, each from where the
    /// last one ended; none find anything if the tree does not exist.
    pub(crate) fn seeker_in(&self, tree: &str) -> Result<Seeker<'_>> {
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        match self.descriptors.find(loader, catalog.as_ref(), name)? {
            Some(descriptor) => Seeker::from_pointer(loader, descriptor.id, descriptor.root),
            None => Seeker::new(loader, 0, None),
        }
    }

    /// Every entry of tree `tree`, in key order.
    pub fn iter(&self, tree: &str) -> Result<Range<'_>> {
        self.range::<&[u8]>(tree, ..)
    }

    /// The entries of tree `tree` whose keys lie within `range`, in key order.
    ///
    /// Any range of byte strings will do: `b"a".as_slice()..b"c".as_slice()`
    /// walks the keys from `a` up to, but not including, `c`, and `key..` every
    /// key from `key` on.
    pub fn range<K: AsRef<[u8]>>(
        &self,
        tree: &str,
        range: impl RangeBounds<K>,
    ) -> Result<Range<'_>> {
        user_tree(tree)?;
        self.range_in(tree, &range, false)
    }

    /// The entries of tree `tree` whose keys lie within `range`, in reverse
    /// key order: the last key first.
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
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        match self.descriptors.find(loader, catalog.as_ref(), name)? {
            Some(descriptor) if !descriptor.root.is_null() => {
                Range::over_committed(loader, descriptor.id, descriptor.root, range, backward)
            }
            _ => Ok(Range::empty()),
        }
    }

    /// The number of entries in tree `tree`, 0 if it does not exist.
    pub fn len(&self, tree: &str) -> Result<u64> {
        user_tree(tree)?;
        self.len_in(tree)
    }

    /// [`len`](Self::len) of any tree, the engine's own included.
    pub(crate) fn len_in(&self, tree: &str) -> Result<u64> {
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        Ok(self
            .descriptors
            .find(loader, catalog.as_ref(), name)?
            .map_or(0, |descriptor| descriptor.entries))
    }

    /// The names of every tree, the engine's own included, in byte order.
    pub(crate) fn tree_names_in(&self) -> Result<Vec<String>> {
        catalog_names(
            &self.shared.loader,
            root_child(self.record.catalog).as_ref(),
        )
    }

    /// The names of every tree, in byte order. The engine's own trees, which
    /// hold the objects of collections, are not among them.
    pub fn tree_names(&self) -> Result<Vec<String>> {
        let mut names = catalog_names(
            &self.shared.loader,
            root_child(self.record.catalog).as_ref(),
        )?;

        names.retain(|name| !engine_tree(name));

        Ok(names)
    }
}

impl Drop for ReadTransaction {
    fn drop(&mut self) {
        self.shared.end_snapshot(self.record.txn);
    }
}

#[cfg(test)]
mod tests {
    use super::KEPT_ROOTS;
    use crate::OpenOptions;

    /// Lookups by key in more trees than a read transaction keeps the roots
    /// of find what its commit holds, the first time and again, while a
    /// writer changes every tree and deletes one; a tree that is empty or
    /// not there holds nothing.
    #[test]
    fn a_reader_looks_up_its_own_commit_from_the_roots_it_keeps() {
        let dir = tempfile::tempdir().unwrap();
        let db = OpenOptions::new()
            .open(dir.path().join("roots.darudb"))
            .unwrap();
        let trees: Vec<String> = (0..KEPT_ROOTS + 4).map(|n| format!("tree {n}")).collect();
        let mut txn = db.begin_write().unwrap();

        for tree in &trees {
            for key in 0..300u32 {
                txn.insert(tree, &key.to_be_bytes(), tree.as_bytes())
                    .unwrap();
            }
        }

        txn.insert("emptied", b"key", b"value").unwrap();
        txn.remove("emptied", b"key").unwrap();
        txn.commit().unwrap();

        let read = db.begin_read().unwrap();
        let mut txn = db.begin_write().unwrap();

        for tree in &trees {
            for key in 0..300u32 {
                txn.insert(tree, &key.to_be_bytes(), b"changed").unwrap();
            }
        }

        txn.delete_tree(&trees[0]).unwrap();
        txn.commit().unwrap();

        let found = |tree: &str, key: &[u8]| {
            let mut value = None;

            let there = read
                .get_in_with(tree, key, &mut |found| {
                    value = Some(found.to_vec());
                    Ok(())
                })
                .unwrap();

            assert_eq!(there, value.is_some());
            value
        };

        for _ in 0..2 {
            for tree in &trees {
                for key in [0u32, 1, 150, 299] {
                    assert_eq!(found(tree, &key.to_be_bytes()).unwrap(), tree.as_bytes());
                }

                assert_eq!(found(tree, &300u32.to_be_bytes()), None);
            }

            assert_eq!(found("emptied", b"key"), None);
            assert_eq!(found("missing", b"key"), None);
        }

        drop(read);

        let read = db.begin_read().unwrap();

        assert_eq!(
            read.get(&trees[1], &0u32.to_be_bytes()).unwrap().unwrap(),
            b"changed"
        );
        assert_eq!(read.get(&trees[0], &0u32.to_be_bytes()).unwrap(), None);
    }
}
