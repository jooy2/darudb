//! Loading committed nodes and overflow values, verified, through the page
//! cache.

use std::collections::{HashMap, VecDeque};
use std::hash::BuildHasherDefault;
use std::sync::{Arc, Mutex, PoisonError};

use super::Load;
use super::node::{LoadedNode, Node};
use crate::error::{Error, Result};
use crate::format::{
    CONTENT_OFFSET, Check, OverflowRef, PageHeader, PageKind, Pointer, content_len,
};
use crate::storage::{Cache, Fold, Pager};

/// The [`Load`] every transaction uses: the pager, and the page cache in
/// front of it.
#[derive(Debug, Clone)]
pub(crate) struct Loader {
    pager: Arc<Pager>,
    cache: Arc<Cache<LoadedNode>>,
    marks: Arc<Mutex<Marks>>,
}

/// The marks walks have worked out for the entries of committed leaves, by
/// page, check and kind: where a field lies in each record of a leaf, for a
/// query that tests or sorts by the field, so that a query run again finds
/// it there rather than stepping over the fields before it in every record.
///
/// They are kept beside the page cache rather than in its nodes, whose
/// fields a lookup reads keep to one cache line; a node grown by a field for
/// them made lookups slower. Like the page cache, they are found only under
/// the check of the page they were worked out from, so a stale entry never
/// matches; the oldest go first once they take more than a sixteenth of the
/// page cache's bytes.
#[derive(Debug, Default)]
struct Marks {
    /// Hashed as the page cache hashes its keys, whose check is a hash of
    /// the page already: hashing them to resist chosen keys took a tenth of
    /// a walk that marked every leaf.
    entries: HashMap<MarksKey, Arc<[u16]>, BuildHasherDefault<Fold>>,
    order: VecDeque<MarksKey>,
    used: usize,
    capacity: usize,
}

/// What marks are kept by: a leaf's page and check, and the kind of mark.
type MarksKey = (u64, Check, u64);

impl Loader {
    pub(crate) fn new(pager: Arc<Pager>, cache: Arc<Cache<LoadedNode>>) -> Self {
        let marks = Marks {
            capacity: cache.capacity() / 16,
            ..Marks::default()
        };

        Self {
            pager,
            cache,
            marks: Arc::new(Mutex::new(marks)),
        }
    }

    /// Whether the page cache holds the node `pointer` names.
    #[cfg(test)]
    pub(crate) fn cached(&self, pointer: &Pointer) -> bool {
        self.cache.get(pointer.page, &pointer.check).is_some()
    }

    /// The bytes the marks kept take.
    #[cfg(test)]
    pub(crate) fn marks_kept(&self) -> usize {
        self.lock_marks().used
    }

    /// A damaged-file error naming this loader's file.
    pub(crate) fn corrupted_file(&self, reason: String) -> Error {
        self.pager.corrupted(reason)
    }

    fn lock_marks(&self) -> std::sync::MutexGuard<'_, Marks> {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn corrupted(&self, page: u64, reason: &str) -> Error {
        self.pager.corrupted(format!("page {page}: {reason}"))
    }

    /// Whether a cached or freshly read node is the one the pointer expects.
    fn check_expected(
        &self,
        page: u64,
        loaded: &LoadedNode,
        pointer: &Pointer,
        tree: u64,
        level: Option<u8>,
    ) -> Result<()> {
        if loaded.txn != pointer.txn {
            return Err(self.corrupted(page, "written by another commit than its parent says"));
        }

        if loaded.tree != tree {
            return Err(self.corrupted(page, "belongs to another tree than its parent's"));
        }

        if level.is_some_and(|level| level != loaded.level()) {
            return Err(self.corrupted(page, "sits at another level than its parent expects"));
        }

        Ok(())
    }
}

impl Load for Loader {
    fn page_size(&self) -> usize {
        self.pager.page_size()
    }

    fn load(&self, pointer: &Pointer, tree: u64, level: Option<u8>) -> Result<Arc<LoadedNode>> {
        let page = pointer.page;

        if let Some(loaded) = self.cache.get(page, &pointer.check) {
            self.check_expected(page, &loaded, pointer, tree, level)?;

            return Ok(loaded);
        }

        let bytes = self.pager.read(page, &pointer.check)?;
        let header = PageHeader::read(&bytes).map_err(|reason| self.corrupted(page, reason))?;
        let loaded = Arc::new(
            LoadedNode::read(bytes, &header).map_err(|reason| self.corrupted(page, reason))?,
        );

        self.check_expected(page, &loaded, pointer, tree, level)?;
        self.cache.insert(page, pointer.check, Arc::clone(&loaded));

        Ok(loaded)
    }

    /// Copying a cached page for every node a transaction first changed took
    /// a tenth of a small deferred commit, and as much of a transaction that
    /// deletes objects. The cache keeps the node its commit made, which the
    /// next transaction takes back, and a node a reader holds is copied, and
    /// put back for the others. A committed page taken is read from the file
    /// again by a reader of an older commit that still needs it.
    fn load_to_change(&self, pointer: &Pointer, tree: u64, level: Option<u8>) -> Result<Node> {
        let page = pointer.page;

        if let Some(loaded) = self.cache.take(page, &pointer.check) {
            self.check_expected(page, &loaded, pointer, tree, level)?;

            return Ok(match Arc::try_unwrap(loaded) {
                Ok(loaded) => loaded.into_node(),
                Err(shared) => {
                    let node = shared.to_node();

                    self.cache.insert(page, pointer.check, shared);
                    node
                }
            });
        }

        let bytes = self.pager.read(page, &pointer.check)?;
        let header = PageHeader::read(&bytes).map_err(|reason| self.corrupted(page, reason))?;
        let loaded =
            LoadedNode::read(bytes, &header).map_err(|reason| self.corrupted(page, reason))?;

        self.check_expected(page, &loaded, pointer, tree, level)?;

        Ok(loaded.into_node())
    }

    fn marks(
        &self,
        pointer: &Pointer,
        kind: u64,
        make: &mut dyn FnMut() -> Option<Arc<[u16]>>,
    ) -> Option<Arc<[u16]>> {
        let key = (pointer.page, pointer.check, kind);

        if let Some(kept) = self.lock_marks().entries.get(&key) {
            return Some(Arc::clone(kept));
        }

        let made = make()?;
        let mut marks = self.lock_marks();
        let weight = size_of_val(&*made);

        if marks.entries.insert(key, Arc::clone(&made)).is_none() {
            marks.order.push_back(key);
            marks.used += weight;
        }

        while marks.used > marks.capacity {
            let Some(oldest) = marks.order.pop_front() else {
                break;
            };

            if let Some(old) = marks.entries.remove(&oldest) {
                marks.used -= size_of_val(&*old);
            }
        }

        Some(made)
    }

    fn read_overflow(&self, reference: &OverflowRef, tree: u64) -> Result<Vec<u8>> {
        let chunk = content_len(self.pager.page_size());
        let len = usize::try_from(reference.len)
            .map_err(|_| self.corrupted(reference.first, "holds a value too large for memory"))?;
        let page_size = self.pager.page_size();
        let mut value = Vec::with_capacity(len);
        let mut checks = Vec::with_capacity(reference.pages as usize * 16);
        let mut index = 0u64;

        // A run of pages at a time, each read with one call.
        while index < u64::from(reference.pages) {
            let left = usize::try_from(u64::from(reference.pages) - index).unwrap_or(usize::MAX);
            let (bytes, run_checks) = self
                .pager
                .read_run_self_checked(reference.first + index, left.min(self.pager.run_pages()))?;

            for (bytes, check) in bytes.chunks(page_size).zip(run_checks) {
                let page = reference.first + index;
                let header =
                    PageHeader::read(bytes).map_err(|reason| self.corrupted(page, reason))?;

                if header.kind != PageKind::Overflow
                    || header.tree != tree
                    || header.txn != reference.txn
                    || header.index != index
                {
                    return Err(
                        self.corrupted(page, "is not the overflow page its reference names")
                    );
                }

                let take = chunk.min(len - value.len());

                value.extend_from_slice(&bytes[CONTENT_OFFSET..CONTENT_OFFSET + take]);
                checks.extend_from_slice(&check.0);
                index += 1;
            }
        }

        if Check::of(&[&checks]) != reference.check {
            return Err(self.corrupted(reference.first, "starts a run that fails its run check"));
        }

        Ok(value)
    }
}
