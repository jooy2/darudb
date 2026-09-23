//! Read transactions.

use std::ops::RangeBounds;
use std::sync::Arc;

use super::{Range, catalog_names, find_tree, root_child, tree_key};
use crate::btree::{self, Load};
use crate::error::Result;
use crate::format::CommitRecord;
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
}

impl ReadTransaction {
    pub(crate) fn begin(shared: &Arc<Shared>) -> Result<Self> {
        let record = shared.begin_snapshot()?;

        Ok(Self {
            shared: Arc::clone(shared),
            record,
        })
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
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        let Some(descriptor) = find_tree(loader, catalog.as_ref(), name)? else {
            return Ok(None);
        };

        btree::get(
            loader,
            descriptor.id,
            root_child(descriptor.root).as_ref(),
            key,
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
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        match find_tree(loader, catalog.as_ref(), name)? {
            Some(descriptor) if !descriptor.root.is_null() => {
                Range::over_committed(loader, descriptor.id, descriptor.root, &range)
            }
            _ => Ok(Range::empty()),
        }
    }

    /// The number of entries in tree `tree`, 0 if it does not exist.
    pub fn len(&self, tree: &str) -> Result<u64> {
        let loader = &self.shared.loader;
        let name = tree_key(tree, loader.page_size())?;
        let catalog = root_child(self.record.catalog);

        Ok(find_tree(loader, catalog.as_ref(), name)?.map_or(0, |descriptor| descriptor.entries))
    }

    /// The names of every tree, in byte order.
    pub fn tree_names(&self) -> Result<Vec<String>> {
        catalog_names(
            &self.shared.loader,
            root_child(self.record.catalog).as_ref(),
        )
    }
}

impl Drop for ReadTransaction {
    fn drop(&mut self) {
        self.shared.end_snapshot(self.record.txn);
    }
}
