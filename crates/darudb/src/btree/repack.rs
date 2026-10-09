//! Repacking: a tree written again in as few pages as its entries fit in,
//! for compaction (`design/tools.md`, "Compaction").
//!
//! Inserts leave pages part empty. A split shares a full page out between
//! two, so keys that come in no particular order keep a tree's leaves about
//! two thirds full, and a tree keeps the pages it had before inserts began to
//! fill them (`insert_into` in `write.rs`). Compaction measures how full the
//! pages of each tree are ([`occupancy`]) and writes again a tree that a full
//! packing would make noticeably smaller ([`repack`]): it reads the entries in
//! order, writes each leaf full before it starts the next, and builds the
//! branches above them the same way, a level at a time. A value in an
//! overflow run stays where it is, and the new leaf takes its reference over,
//! so a repack writes the tree's nodes and nothing else.
//!
//! The old nodes are released as they are read, as a deleted tree's are, and
//! the commit retains them like any page it stops using. A full leaf splits
//! at the next insert into it, as a full leaf always does, and the run of
//! inserts that grows a range goes on filling the leaves it makes.

use std::mem;
use std::sync::Arc;

use super::leaf::Leaf;
use super::node::{Branch, Child, Keys, Node, NodeRef};
use super::read::internal;
use super::{Load, Store};
use crate::error::Result;
use crate::format::{Cells, LeafEntry, POINTER_LEN, Pointer, branch_key_len, content_len};

/// How full the pages of a tree are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Occupancy {
    /// The pages its nodes take.
    pub(crate) pages: u64,
    /// The bytes of those pages' content areas its entries and keys take.
    pub(crate) used: u64,
}

impl Occupancy {
    /// Whether packing the tree would save a tenth of its pages or more,
    /// which is worth writing it again for, and at least two pages.
    pub(crate) fn worth_repacking(&self, page_size: usize) -> bool {
        let capacity = content_len(page_size) as u64;
        let packed = self.used.div_ceil(capacity);

        self.pages >= packed + 2 && packed * 10 <= self.pages * 9
    }
}

/// How full the pages of the committed tree under `root` are, its leaves
/// measured in `cells`: a file raised to format 6 repacks the trees whose
/// leaves of format 5 the smaller cells would take noticeably fewer pages
/// of.
pub(crate) fn occupancy<L: Load>(
    load: &L,
    tree: u64,
    root: &Pointer,
    cells: Cells,
) -> Result<Occupancy> {
    let mut occupancy = Occupancy::default();
    let mut pending = vec![(*root, None)];

    while let Some((pointer, level)) = pending.pop() {
        let loaded = load.load(&pointer, tree, level)?;
        let node = NodeRef::Loaded(Arc::clone(&loaded));

        occupancy.pages += 1;
        occupancy.used += node.size_in(cells)? as u64;

        if !node.is_leaf() {
            let below = Some(loaded.level() - 1);

            pending.extend((0..=node.count()).map(|index| (loaded.child(index), below)));
        }
    }

    Ok(occupancy)
}

/// Writes the tree under `root` again, each leaf full before the next and
/// each branch full before the next, and releases its old nodes. The tree
/// holds the same entries, in the same order, under the same overflow runs.
pub(crate) fn repack<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
) -> Result<()> {
    let Some(old) = root.take() else {
        return Ok(());
    };
    let page_size = load.page_size();
    let capacity = content_len(page_size);
    let cells = store.cells();
    let mut leaves = Vec::new();
    let mut entries: Vec<LeafEntry> = Vec::new();
    let mut size = 0;
    // Depth first, the first child last onto the stack, so that the leaves
    // come in the order of their keys.
    let mut pending = vec![(old, None)];

    while let Some((child, level)) = pending.pop() {
        let node = match child {
            Child::Clean(pointer) => {
                let node = load.node_to_change(store, &pointer, tree, level)?;

                store.release(pointer.page, pointer.txn);
                node
            }
            Child::Dirty { page, node } => {
                store.release(page, store.txn());
                *node
            }
        };

        match node {
            Node::Leaf(leaf) => {
                for entry in leaf.to_entries().map_err(internal)? {
                    if size + entry.len(cells) > capacity && !entries.is_empty() {
                        leaves.push(write_leaf(store, page_size, &mut entries)?);
                        size = 0;
                    }

                    size += entry.len(cells);
                    entries.push(entry);
                }
            }
            Node::Branch(branch) => {
                let below = Some(branch.level - 1);

                pending.extend(
                    branch
                        .children
                        .into_iter()
                        .rev()
                        .map(|child| (child, below)),
                );
            }
        }
    }

    if !entries.is_empty() {
        leaves.push(write_leaf(store, page_size, &mut entries)?);
    }

    let mut nodes = leaves;
    let mut level = 1;

    while nodes.len() > 1 {
        nodes = write_branches(store, level, nodes, capacity)?;
        level += 1;
    }

    *root = nodes.pop().map(|(_, child)| child);

    Ok(())
}

/// A new leaf of `entries`, which it takes, with its first key.
fn write_leaf<S: Store>(
    store: &mut S,
    page_size: usize,
    entries: &mut Vec<LeafEntry>,
) -> Result<(Vec<u8>, Child)> {
    let leaf = Leaf::from_entries(page_size, store.cells(), entries);
    let first = mem::take(&mut entries[0].key);
    let page = store.allocate()?;

    entries.clear();

    Ok((first, Child::dirty(page, Node::Leaf(leaf))))
}

/// The branches of `level` above `nodes`, given with their first keys, each
/// full before the next starts, with their own first keys.
fn write_branches<S: Store>(
    store: &mut S,
    level: u8,
    nodes: Vec<(Vec<u8>, Child)>,
    capacity: usize,
) -> Result<Vec<(Vec<u8>, Child)>> {
    let mut groups: Vec<Vec<(Vec<u8>, Child)>> = Vec::new();
    let mut size = 0;

    for (key, child) in nodes {
        let grown = size + branch_key_len(key.len());

        match groups.last_mut() {
            // The first child of a branch takes a pointer and no key.
            Some(group) if grown <= capacity => {
                group.push((key, child));
                size = grown;
            }
            _ => {
                groups.push(vec![(key, child)]);
                size = POINTER_LEN;
            }
        }
    }

    // A branch has two children at least: a last one with one takes the
    // last child of the full branch before it.
    if let [.., before, last] = groups.as_mut_slice() {
        if last.len() == 1 {
            let moved = before
                .pop()
                .ok_or_else(|| internal("an empty branch while repacking"))?;

            last.insert(0, moved);
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let mut children = Vec::with_capacity(group.len());
            let mut first = None;
            let keys = {
                let mut keys = Vec::with_capacity(group.len().saturating_sub(1));

                for (key, child) in group {
                    if first.is_none() {
                        first = Some(key);
                    } else {
                        keys.push(key);
                    }

                    children.push(child);
                }

                Keys::of(keys.iter().map(Vec::as_slice))
            };
            let first = first.ok_or_else(|| internal("an empty branch while repacking"))?;
            let page = store.allocate()?;

            Ok((
                first,
                Child::dirty(
                    page,
                    Node::Branch(Branch {
                        level,
                        keys,
                        children,
                    }),
                ),
            ))
        })
        .collect()
}
