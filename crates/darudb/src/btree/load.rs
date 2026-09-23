//! Loading committed nodes and overflow values, verified, through the page
//! cache.

use std::sync::Arc;

use super::Load;
use super::node::{LoadedNode, Node};
use crate::error::{Error, Result};
use crate::format::{
    CONTENT_OFFSET, Check, OverflowRef, PageHeader, PageKind, Pointer, content_len,
};
use crate::storage::{Cache, Pager};

/// The [`Load`] every transaction uses: the pager, and the page cache in
/// front of it.
#[derive(Debug, Clone)]
pub(crate) struct Loader {
    pager: Arc<Pager>,
    cache: Arc<Cache<LoadedNode>>,
}

impl Loader {
    pub(crate) fn new(pager: Arc<Pager>, cache: Arc<Cache<LoadedNode>>) -> Self {
        Self { pager, cache }
    }

    /// A damaged-file error naming this loader's file.
    pub(crate) fn corrupted_file(&self, reason: String) -> Error {
        self.pager.corrupted(reason)
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

        if level.is_some_and(|level| level != loaded.node.level()) {
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
        let node = Node::decode(&bytes, &header).map_err(|reason| self.corrupted(page, reason))?;
        let loaded = Arc::new(LoadedNode {
            tree: header.tree,
            txn: header.txn,
            node,
        });

        self.check_expected(page, &loaded, pointer, tree, level)?;
        self.cache.insert(page, pointer.check, Arc::clone(&loaded));

        Ok(loaded)
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
