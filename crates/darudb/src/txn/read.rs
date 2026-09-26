//! Read transactions.

use std::ops::RangeBounds;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{Descriptors, Range, catalog_names, engine_tree, root_child, tree_key, user_tree};
use crate::btree::{self, Load};
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
}

impl ReadTransaction {
    pub(crate) fn begin(shared: &Arc<Shared>, schema: Option<Arc<OpenSchema>>) -> Result<Self> {
        let record = shared.begin_snapshot()?;

        Ok(Self {
            shared: Arc::clone(shared),
            record,
            schema,
            schema_checked: AtomicBool::new(false),
            descriptors: Descriptors::shared(shared.learned(record.txn)),
        })
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
        let catalog = root_child(self.record.catalog);

        let Some(descriptor) = self.descriptors.find(loader, catalog.as_ref(), name)? else {
            return Ok(false);
        };

        btree::get_with(
            loader,
            descriptor.id,
            root_child(descriptor.root).as_ref(),
            key,
            visit,
        )
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
