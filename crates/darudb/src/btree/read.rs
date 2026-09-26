//! Looking keys up and walking ranges of keys.

use std::cmp::Ordering;
use std::ops::Bound;

use super::Load;
use super::node::{Child, Node, NodeRef, compare};
use crate::error::{Error, Result};
use crate::format::{Pointer, StoredRef, StoredValue};

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
        NodeRef::Borrowed(Node::Branch(branch)) => {
            resolve(load, &branch.children[index], tree, Some(branch.level - 1))
        }
        NodeRef::Loaded(loaded) if !node.is_leaf() => Ok(NodeRef::Loaded(load.load(
            &loaded.child(index),
            tree,
            Some(loaded.level() - 1),
        )?)),
        _ => Err(internal("descended into a leaf")),
    }
}

/// An error that only a bug in the engine can cause.
pub(super) fn internal(what: &str) -> Error {
    Error::Internal {
        message: what.to_owned(),
    }
}

/// The leaf that holds `key`, and where in it, if the tree has the key.
fn find<'a, L: Load>(
    load: &L,
    tree: u64,
    root: Option<&'a Child>,
    key: &[u8],
) -> Result<Option<(NodeRef<'a>, usize)>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let mut node = resolve(load, root, tree, None)?;

    loop {
        if node.is_leaf() {
            let index = node.rank(key, false);

            if index == node.count() || node.key(index) != key {
                return Ok(None);
            }

            return Ok(Some((node, index)));
        }

        let index = node.rank(key, true);

        node = descend(load, &node, tree, index)?;
    }
}

/// Whether `key` is stored, found without copying its value.
pub(crate) fn contains<L: Load>(
    load: &L,
    tree: u64,
    root: Option<&Child>,
    key: &[u8],
) -> Result<bool> {
    Ok(find(load, tree, root, key)?.is_some())
}

/// The value stored under `key`, as the leaf stores it.
pub(crate) fn get_stored<L: Load>(
    load: &L,
    tree: u64,
    root: Option<&Child>,
    key: &[u8],
) -> Result<Option<StoredValue>> {
    let Some((node, index)) = find(load, tree, root, key)? else {
        return Ok(None);
    };

    Ok(Some(match node.value(index)? {
        StoredRef::Inline(value) => StoredValue::Inline(value.to_vec()),
        StoredRef::Overflow(reference) => StoredValue::Overflow(reference),
    }))
}

/// Gives `visit` the value stored under `key`, borrowed from its leaf when
/// the leaf holds it, and returns whether there was one. A caller that only
/// reads the value copies nothing.
pub(crate) fn get_with<L: Load>(
    load: &L,
    tree: u64,
    root: Option<&Child>,
    key: &[u8],
    visit: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let Some((node, index)) = find(load, tree, root, key)? else {
        return Ok(false);
    };

    match node.value(index)? {
        StoredRef::Inline(value) => visit(value)?,
        StoredRef::Overflow(reference) => visit(&load.read_overflow(&reference, tree)?)?,
    }

    Ok(true)
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

/// Lookups of one key after another in one tree, each going down from the
/// deepest node of the last one's path whose range holds the new key, rather
/// than from the root.
///
/// A query reads the objects an index names in the index's order, which for
/// one value of the index is the order of their keys, so one lookup after
/// another lands in a leaf near the last: going down from the root each
/// time looked up the same upper nodes in the page cache again and searched
/// them again.
pub(crate) struct Seeker<'a, L: Load> {
    load: &'a L,
    tree: u64,
    root: Option<NodeRef<'a>>,
    /// The branches from the root to the last leaf found, each with the
    /// child the path took.
    path: Vec<(NodeRef<'a>, usize)>,
    /// The last leaf found.
    leaf: Option<NodeRef<'a>>,
}

impl<'a, L: Load> Seeker<'a, L> {
    /// Lookups in the tree rooted at `root`, empty with no root.
    pub(crate) fn new(load: &'a L, tree: u64, root: Option<&'a Child>) -> Result<Self> {
        let root = root
            .map(|root| resolve(load, root, tree, None))
            .transpose()?;

        Ok(Self::rooted(load, tree, root))
    }

    /// Lookups in the committed tree rooted at `root`, empty for the null
    /// pointer.
    pub(crate) fn from_pointer(load: &'a L, tree: u64, root: Pointer) -> Result<Self> {
        let root = if root.is_null() {
            None
        } else {
            Some(NodeRef::Loaded(load.load(&root, tree, None)?))
        };

        Ok(Self::rooted(load, tree, root))
    }

    fn rooted(load: &'a L, tree: u64, root: Option<NodeRef<'a>>) -> Self {
        Self {
            load,
            tree,
            root,
            path: Vec::new(),
            leaf: None,
        }
    }

    /// [`get_with`]: gives `visit` the value stored under `key`, borrowed
    /// when the leaf holds it, and returns whether there was one.
    pub(crate) fn get_with(
        &mut self,
        key: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        // Every node on the path holds the key while each child it took does;
        // the first child that does not is where the path turns.
        let kept = self
            .path
            .iter()
            .take_while(|(node, child)| holds(node, *child, key))
            .count();
        let turn = self.path.get(kept).map(|(node, _)| node.clone());
        let mut node = match (turn, self.leaf.take()) {
            (Some(node), _) => {
                self.path.truncate(kept);
                node
            }
            (None, Some(leaf)) => leaf,
            // The first lookup, or one after a lookup that failed on its way
            // down.
            (None, None) => {
                self.path.clear();

                match &self.root {
                    Some(root) => root.clone(),
                    None => return Ok(false),
                }
            }
        };

        while !node.is_leaf() {
            let index = node.rank(key, true);
            let child = descend(self.load, &node, self.tree, index)?;

            self.path.push((node, index));
            node = child;
        }

        let leaf = self.leaf.insert(node);
        let index = leaf.rank(key, false);

        if index == leaf.count() || leaf.key(index) != key {
            return Ok(false);
        }

        match leaf.value(index)? {
            StoredRef::Inline(value) => visit(value)?,
            StoredRef::Overflow(reference) => {
                visit(&self.load.read_overflow(&reference, self.tree)?)?;
            }
        }

        Ok(true)
    }

    /// The branches on the path to the last leaf found.
    #[cfg(test)]
    pub(crate) fn depth(&self) -> usize {
        self.path.len()
    }
}

/// Whether child `child` of the branch `node` holds `key`: whether `key` lies
/// from the separator before the child up to, but not including, the one
/// after it.
fn holds(node: &NodeRef<'_>, child: usize, key: &[u8]) -> bool {
    (child == 0 || compare(node.key(child - 1), key) != Ordering::Greater)
        && (child == node.count() || compare(key, node.key(child)) == Ordering::Less)
}

/// What [`Range::for_each`] gives each entry to: a key and a value, borrowed.
/// It returns whether to stop.
pub(crate) type Visit<'v> = dyn FnMut(&[u8], &[u8]) -> Result<bool> + 'v;

/// The entries of a tree from one bound to another, in key order or, walking
/// backwards, in reverse.
#[derive(Debug)]
pub(crate) struct Range<'a, L: Load> {
    load: &'a L,
    tree: u64,
    /// The path from the root to the current leaf. For a branch, the index is
    /// the child being walked. For the leaf, it is the next entry walking
    /// forwards, and one past it walking backwards.
    stack: Vec<(NodeRef<'a>, usize)>,
    /// Where the walk stops: the end bound forwards, the start bound
    /// backwards.
    stop: Bound<Vec<u8>>,
    backward: bool,
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
        Self::walk(load, tree, root, start, end, false)
    }

    /// Positions a walk at the last key within `end`, to go backwards down to
    /// `start`.
    pub(crate) fn new_backward(
        load: &'a L,
        tree: u64,
        root: Option<&'a Child>,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Result<Self> {
        Self::walk(load, tree, root, start, end, true)
    }

    fn walk(
        load: &'a L,
        tree: u64,
        root: Option<&'a Child>,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Result<Self> {
        let mut range = Self::unpositioned(load, tree, start, end, backward);
        let Some(root) = root else {
            return Ok(range);
        };
        let node = resolve(load, root, tree, None)?;

        range.position(node, start, end)?;

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
        Self::walk_pointer(load, tree, root, start, end, false)
    }

    /// Positions a backward walk of the committed tree rooted at `root`.
    pub(crate) fn from_pointer_backward(
        load: &'a L,
        tree: u64,
        root: Pointer,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Result<Self> {
        Self::walk_pointer(load, tree, root, start, end, true)
    }

    fn walk_pointer(
        load: &'a L,
        tree: u64,
        root: Pointer,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Result<Self> {
        let mut range = Self::unpositioned(load, tree, start, end, backward);

        if !root.is_null() {
            let node = NodeRef::Loaded(load.load(&root, tree, None)?);

            range.position(node, start, end)?;
        }

        Ok(range)
    }

    fn unpositioned(
        load: &'a L,
        tree: u64,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Self {
        let stop = if backward { start } else { end };

        Self {
            load,
            tree,
            stack: Vec::new(),
            stop: stop.map(<[u8]>::to_vec),
            backward,
        }
    }

    fn position(
        &mut self,
        node: NodeRef<'a>,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Result<()> {
        if self.backward {
            self.seek_back(node, end)
        } else {
            self.seek(node, start)
        }
    }

    /// Descends from `node` to the first entry within `start`.
    fn seek(&mut self, mut node: NodeRef<'a>, start: Bound<&[u8]>) -> Result<()> {
        let load = self.load;
        let tree = self.tree;

        loop {
            if node.is_leaf() {
                let index = match start {
                    Bound::Unbounded => 0,
                    Bound::Included(key) => node.rank(key, false),
                    Bound::Excluded(key) => node.rank(key, true),
                };

                self.stack.push((node, index));

                return Ok(());
            }

            let index = match start {
                Bound::Unbounded => 0,
                Bound::Included(key) | Bound::Excluded(key) => node.rank(key, true),
            };
            let child = descend(load, &node, tree, index)?;

            self.stack.push((node, index));
            node = child;
        }
    }

    /// Descends from `node` to just past the last entry within `end`.
    fn seek_back(&mut self, mut node: NodeRef<'a>, end: Bound<&[u8]>) -> Result<()> {
        let load = self.load;
        let tree = self.tree;

        loop {
            if node.is_leaf() {
                let index = match end {
                    Bound::Unbounded => node.count(),
                    Bound::Included(key) => node.rank(key, true),
                    Bound::Excluded(key) => node.rank(key, false),
                };

                self.stack.push((node, index));

                return Ok(());
            }

            // A branch has one more child than keys.
            let index = match end {
                Bound::Unbounded => node.count(),
                Bound::Included(key) | Bound::Excluded(key) => node.rank(key, true),
            };
            let child = descend(load, &node, tree, index)?;

            self.stack.push((node, index));
            node = child;
        }
    }

    /// Moves from an exhausted leaf, walking backwards, to just past the end
    /// of the last leaf before it.
    fn previous_leaf(&mut self) -> Result<()> {
        self.stack.pop();

        while let Some((node, index)) = self.stack.last_mut() {
            if node.is_leaf() {
                return Err(internal("a leaf above a leaf"));
            }

            if *index == 0 {
                self.stack.pop();

                continue;
            }

            *index -= 1;

            let (parent, index) = (node.clone(), *index);
            let mut child = descend(self.load, &parent, self.tree, index)?;

            loop {
                // The last child of a branch, or just past a leaf's last
                // entry: one past the last key either way.
                let last = child.count();

                if child.is_leaf() {
                    self.stack.push((child, last));

                    return Ok(());
                }

                let next = descend(self.load, &child, self.tree, last)?;

                self.stack.push((child, last));
                child = next;
            }
        }

        Ok(())
    }

    /// Moves from an exhausted leaf to the first leaf after it.
    fn next_leaf(&mut self) -> Result<()> {
        self.stack.pop();

        while let Some((node, index)) = self.stack.last_mut() {
            if node.is_leaf() {
                return Err(internal("a leaf above a leaf"));
            }

            if *index + 1 > node.count() {
                self.stack.pop();

                continue;
            }

            *index += 1;

            let (parent, index) = (node.clone(), *index);
            let mut child = descend(self.load, &parent, self.tree, index)?;

            loop {
                if child.is_leaf() {
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

            if !node.is_leaf() {
                return Err(internal("a range stopped on a branch"));
            }

            let at = if self.backward {
                if *index == 0 {
                    self.previous_leaf()?;

                    continue;
                }

                *index -= 1;
                *index
            } else {
                if *index == node.count() {
                    self.next_leaf()?;

                    continue;
                }

                *index += 1;
                *index - 1
            };
            let node = node.clone();
            let (key, value) = node.entry(at)?;

            if self.beyond_stop(key) {
                self.stack.clear();

                return Ok(None);
            }

            let value = match value {
                StoredRef::Inline(value) => value.to_vec(),
                StoredRef::Overflow(reference) => self.load.read_overflow(&reference, self.tree)?,
            };

            return Ok(Some((key.to_vec(), value)));
        }
    }

    /// The entries of the leaf on top of the stack that the walk has left,
    /// as the range of their indexes, and whether the walk stops within the
    /// leaf. The keys are in order, so the ones past the stop are at one end
    /// of the leaf, where a binary search finds them.
    fn left_in_leaf(&self) -> Result<Option<(NodeRef<'a>, usize, usize, bool)>> {
        let Some((node, index)) = self.stack.last() else {
            return Ok(None);
        };

        if !node.is_leaf() {
            return Err(internal("a range stopped on a branch"));
        }

        let (from, to) = if self.backward {
            (0, *index)
        } else {
            (*index, node.count())
        };
        let (mut low, mut high) = (from, to);

        if !matches!(self.stop, Bound::Unbounded) {
            while low < high {
                let middle = low + (high - low) / 2;

                if self.beyond_stop(node.key(middle)) == self.backward {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
        } else if self.backward {
            low = from;
        } else {
            low = to;
        }

        Ok(Some(if self.backward {
            (node.clone(), low, to, low > from)
        } else {
            (node.clone(), from, low, low < to)
        }))
    }

    /// The number of entries the walk has left, counted a leaf at a time
    /// rather than an entry at a time, and without copying any.
    pub(crate) fn count_entries(mut self) -> Result<u64> {
        let mut count = 0u64;

        while let Some((_, from, to, done)) = self.left_in_leaf()? {
            count += (to - from) as u64;

            if done {
                break;
            }

            if self.backward {
                self.previous_leaf()?;
            } else {
                self.next_leaf()?;
            }
        }

        Ok(count)
    }

    /// Gives each entry the walk has left to `visit`, borrowed from its leaf,
    /// until `visit` returns true. Nothing is copied out of a leaf, except a
    /// value kept in overflow pages, which is read.
    pub(crate) fn for_each(mut self, visit: &mut Visit<'_>) -> Result<()> {
        while let Some((node, from, to, done)) = self.left_in_leaf()? {
            let mut give = |at: usize| -> Result<bool> {
                if let Some((key, value)) = node.inline_entry(at) {
                    return visit(key, value);
                }

                match node.entry(at)? {
                    (key, StoredRef::Inline(value)) => visit(key, value),
                    (key, StoredRef::Overflow(reference)) => {
                        visit(key, &self.load.read_overflow(&reference, self.tree)?)
                    }
                }
            };

            if self.backward {
                for at in (from..to).rev() {
                    if give(at)? {
                        return Ok(());
                    }
                }
            } else {
                for at in from..to {
                    if give(at)? {
                        return Ok(());
                    }
                }
            }

            if done {
                return Ok(());
            }

            if self.backward {
                self.previous_leaf()?;
            } else {
                self.next_leaf()?;
            }
        }

        Ok(())
    }

    /// Whether `key` lies past the bound the walk stops at: beyond the end
    /// forwards, before the start backwards.
    fn beyond_stop(&self, key: &[u8]) -> bool {
        match (&self.stop, self.backward) {
            (Bound::Unbounded, _) => false,
            (Bound::Included(stop), false) => key > stop.as_slice(),
            (Bound::Excluded(stop), false) => key >= stop.as_slice(),
            (Bound::Included(stop), true) => key < stop.as_slice(),
            (Bound::Excluded(stop), true) => key <= stop.as_slice(),
        }
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
