//! Looking keys up and walking ranges of keys.

use std::ops::Bound;

use super::Load;
use super::node::{Child, Node, NodeRef, child_index};
use crate::error::{Error, Result};
use crate::format::{Pointer, StoredValue};

/// The node `child` refers to: borrowed if this transaction holds it, loaded
/// and verified otherwise.
pub(crate) fn resolve<'a, L: Load>(
    load: &L,
    child: &'a Child,
    tree: u64,
    level: Option<u8>,
) -> Result<NodeRef<'a>> {
    match child {
        Child::Clean(pointer) => Ok(NodeRef::Loaded(load.load(pointer, tree, level)?)),
        Child::Dirty { node, .. } => Ok(NodeRef::Borrowed(node)),
    }
}

/// Child `index` of the branch `node`.
fn descend<'a, L: Load>(
    load: &L,
    node: &NodeRef<'a>,
    tree: u64,
    index: usize,
) -> Result<NodeRef<'a>> {
    match node {
        NodeRef::Borrowed(node) => match node {
            Node::Branch(branch) => {
                resolve(load, &branch.children[index], tree, Some(branch.level - 1))
            }
            Node::Leaf(_) => Err(internal("descended into a leaf")),
        },
        NodeRef::Loaded(loaded) => match &loaded.node {
            Node::Branch(branch) => match &branch.children[index] {
                Child::Clean(pointer) => Ok(NodeRef::Loaded(load.load(
                    pointer,
                    tree,
                    Some(branch.level - 1),
                )?)),
                Child::Dirty { .. } => Err(internal("a committed node holds a dirty child")),
            },
            Node::Leaf(_) => Err(internal("descended into a leaf")),
        },
    }
}

/// An error that only a bug in the engine can cause.
pub(super) fn internal(what: &str) -> Error {
    Error::Internal {
        message: what.to_owned(),
    }
}

/// The value stored under `key`, as the leaf stores it.
pub(crate) fn get_stored<L: Load>(
    load: &L,
    tree: u64,
    root: Option<&Child>,
    key: &[u8],
) -> Result<Option<StoredValue>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let mut node = resolve(load, root, tree, None)?;

    loop {
        let index = match &*node {
            Node::Leaf(entries) => {
                return Ok(entries
                    .binary_search_by(|entry| entry.key.as_slice().cmp(key))
                    .ok()
                    .map(|index| entries[index].value.clone()));
            }
            Node::Branch(branch) => child_index(&branch.keys, key),
        };

        node = descend(load, &node, tree, index)?;
    }
}

/// The value stored under `key`.
pub(crate) fn get<L: Load>(
    load: &L,
    tree: u64,
    root: Option<&Child>,
    key: &[u8],
) -> Result<Option<Vec<u8>>> {
    match get_stored(load, tree, root, key)? {
        None => Ok(None),
        Some(StoredValue::Inline(value)) => Ok(Some(value)),
        Some(StoredValue::Overflow(reference)) => Ok(Some(load.read_overflow(&reference, tree)?)),
    }
}

/// The entries of a tree from one bound to another, in key order.
#[derive(Debug)]
pub(crate) struct Range<'a, L: Load> {
    load: &'a L,
    tree: u64,
    /// The path from the root to the current leaf. For a branch, the index is
    /// the child being walked; for the leaf, the next entry.
    stack: Vec<(NodeRef<'a>, usize)>,
    end: Bound<Vec<u8>>,
}

impl<'a, L: Load> Range<'a, L> {
    /// Positions a walk at the first key within `start`.
    pub(crate) fn new(
        load: &'a L,
        tree: u64,
        root: Option<&'a Child>,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Result<Self> {
        let mut range = Self {
            load,
            tree,
            stack: Vec::new(),
            end: end.map(<[u8]>::to_vec),
        };
        let Some(root) = root else {
            return Ok(range);
        };
        let node = resolve(load, root, tree, None)?;

        range.seek(node, start)?;

        Ok(range)
    }

    /// Positions a walk of the committed tree rooted at `root`.
    pub(crate) fn from_pointer(
        load: &'a L,
        tree: u64,
        root: Pointer,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Result<Self> {
        let mut range = Self {
            load,
            tree,
            stack: Vec::new(),
            end: end.map(<[u8]>::to_vec),
        };

        if !root.is_null() {
            let node = NodeRef::Loaded(load.load(&root, tree, None)?);

            range.seek(node, start)?;
        }

        Ok(range)
    }

    /// Descends from `node` to the first entry within `start`.
    fn seek(&mut self, mut node: NodeRef<'a>, start: Bound<&[u8]>) -> Result<()> {
        let load = self.load;
        let tree = self.tree;

        loop {
            match &*node {
                Node::Branch(branch) => {
                    let index = match start {
                        Bound::Unbounded => 0,
                        Bound::Included(key) | Bound::Excluded(key) => {
                            child_index(&branch.keys, key)
                        }
                    };
                    let child = descend(load, &node, tree, index)?;

                    self.stack.push((node, index));
                    node = child;
                }
                Node::Leaf(entries) => {
                    let index = match start {
                        Bound::Unbounded => 0,
                        Bound::Included(key) => {
                            entries.partition_point(|entry| entry.key.as_slice() < key)
                        }
                        Bound::Excluded(key) => {
                            entries.partition_point(|entry| entry.key.as_slice() <= key)
                        }
                    };

                    self.stack.push((node, index));

                    return Ok(());
                }
            }
        }
    }

    /// Moves from an exhausted leaf to the first leaf after it.
    fn next_leaf(&mut self) -> Result<()> {
        self.stack.pop();

        while let Some((node, index)) = self.stack.last_mut() {
            let Node::Branch(branch) = &**node else {
                return Err(internal("a leaf above a leaf"));
            };

            if *index + 1 >= branch.children.len() {
                self.stack.pop();

                continue;
            }

            *index += 1;

            let (parent, index) = (node.clone(), *index);
            let mut child = descend(self.load, &parent, self.tree, index)?;

            loop {
                let is_branch = matches!(&*child, Node::Branch(_));

                if !is_branch {
                    self.stack.push((child, 0));

                    return Ok(());
                }

                let next = descend(self.load, &child, self.tree, 0)?;

                self.stack.push((child, 0));
                child = next;
            }
        }

        Ok(())
    }

    fn step(&mut self) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        loop {
            let Some((node, index)) = self.stack.last_mut() else {
                return Ok(None);
            };
            let Node::Leaf(entries) = &**node else {
                return Err(internal("a range stopped on a branch"));
            };

            if *index < entries.len() {
                let entry = entries[*index].clone();

                *index += 1;

                if past_end(&self.end, &entry.key) {
                    self.stack.clear();

                    return Ok(None);
                }

                let value = match entry.value {
                    StoredValue::Inline(value) => value,
                    StoredValue::Overflow(reference) => {
                        self.load.read_overflow(&reference, self.tree)?
                    }
                };

                return Ok(Some((entry.key, value)));
            }

            self.next_leaf()?;
        }
    }
}

/// Whether `key` lies beyond the end bound of a range.
fn past_end(end: &Bound<Vec<u8>>, key: &[u8]) -> bool {
    match end {
        Bound::Unbounded => false,
        Bound::Included(end) => key > end.as_slice(),
        Bound::Excluded(end) => key >= end.as_slice(),
    }
}

impl<L: Load> Iterator for Range<'_, L> {
    type Item = Result<(Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.step() {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => None,
            Err(error) => {
                // A damaged page ends the walk: it cannot be stepped over.
                self.stack.clear();

                Some(Err(error))
            }
        }
    }
}
