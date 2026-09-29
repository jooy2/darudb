//! Copy-on-write B+trees over the pages of one file.
//!
//! A committed page is never changed. A write transaction copies the nodes it
//! changes into pages of its own, which it holds in memory as [`Child::Dirty`]
//! until the commit writes them with [`finish`]. Every committed node is read
//! through a [`Load`], which verifies the page against the pointer that led to
//! it before anything in it is used.
//!
//! Keys are ordered by comparing bytes as unsigned numbers. The engine knows
//! no other order; a layer that stores typed keys encodes them so that byte
//! order is the order it wants.

mod finish;
mod leaf;
mod load;
mod node;
mod read;
#[cfg(test)]
mod tests;
mod write;

use std::sync::Arc;

use crate::error::Result;
use crate::format::{Check, OverflowRef, Pointer};

pub(crate) use finish::{FinishedPage, finish};
pub(crate) use load::Loader;
pub(crate) use node::{Child, LoadedNode, Node, NodeRef};
pub(crate) use read::{Range, Seeker, Visit, get, get_with, resolve};
pub(crate) use write::{
    Change, Inserted, Removed, delete_tree, insert, insert_with, relocate, remove, remove_present,
    remove_with, update_with,
};

/// Where committed nodes and overflow values come from.
pub(crate) trait Load {
    /// The size of every page, in bytes.
    fn page_size(&self) -> usize;

    /// Loads the committed node `pointer` names, verified against the pointer,
    /// the tree `tree`, and the `level` its parent expects (`None` for a root).
    fn load(&self, pointer: &Pointer, tree: u64, level: Option<u8>) -> Result<Arc<LoadedNode>>;

    /// [`load`](Self::load) for a write transaction that changes the node,
    /// or lets it go: the node decoded into one of its own, taken out of the
    /// page cache without a copy when nothing else holds it there.
    fn load_to_change(&self, pointer: &Pointer, tree: u64, level: Option<u8>) -> Result<Node>;

    /// The node `pointer` names, decoded for a write transaction to change,
    /// as `store` has it change a node: taken from the page cache when its
    /// page is young, and copied otherwise.
    ///
    /// A young page is one the next commits of the window write again, and
    /// taking it saves the copy a small deferred commit made of every node
    /// it changed. An older page is the state readers go on reading, and a
    /// transaction that took it and was then dropped would have left the
    /// cache without it.
    fn node_to_change<S: Store>(
        &self,
        store: &S,
        pointer: &Pointer,
        tree: u64,
        level: Option<u8>,
    ) -> Result<Node> {
        if pointer.txn > store.young_after() {
            self.load_to_change(pointer, tree, level)
        } else {
            Ok(self.load(pointer, tree, level)?.to_node())
        }
    }

    /// Reads a value stored in an overflow run, verified against its
    /// reference.
    fn read_overflow(&self, reference: &OverflowRef, tree: u64) -> Result<Vec<u8>>;
}

/// Where a write transaction gets and gives back pages.
pub(crate) trait Store {
    /// The transaction id of the commit being made.
    fn txn(&self) -> u64;

    /// Pages written by a commit after this id are young: written in the
    /// unsynced window, which the next commits write again.
    fn young_after(&self) -> u64;

    /// A free page for this transaction.
    fn allocate(&mut self) -> Result<u64>;

    /// `pages` consecutive free pages for this transaction; returns the first.
    fn allocate_run(&mut self, pages: u64) -> Result<u64>;

    /// Gives back a page this transaction no longer uses, which transaction
    /// `written` wrote: the id in the pointer that led to it, or this
    /// transaction's for a page it allocated. A page it allocated itself
    /// becomes free again; a committed page is retained until no reader and
    /// no recovery can need it.
    fn release(&mut self, page: u64, written: u64);

    /// Seals and writes consecutive pages from `first` on right away, in one
    /// call, and returns their checks. Used for overflow runs, whose content
    /// is final as soon as it is known.
    fn write_run(&mut self, first: u64, bytes: &mut [u8]) -> Result<Vec<Check>>;

    /// How many pages [`write_run`](Self::write_run) should get at most.
    fn run_pages(&self) -> usize;
}
