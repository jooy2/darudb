//! Changing a tree, copy-on-write.
//!
//! The first change to a committed node copies it into a page of the
//! transaction ([`make_dirty`]); later changes in the same transaction change
//! that copy in place. A node that grows past its page is split in two. A node
//! that shrinks below a quarter of its page is merged with a neighbour, or,
//! when the two do not fit in one page, the entries of both are shared out
//! again evenly.

use std::mem;

use super::leaf::Leaf;
use super::node::{Branch, Child, Keys, Node};
use super::read::{contains, contains_below, internal};
use super::{Load, Store};
use crate::error::Result;
use crate::format::{
    CONTENT_OFFSET, Check, LeafEntry, OverflowRef, POINTER_LEN, PageHeader, PageKind, Pointer,
    StoredRef, branch_key_len, cell_len, content_len, inline_entry_len, inline_limit,
    overflow_pages,
};

/// A node split in two: the separator and the new right-hand node.
type Split = Option<(Vec<u8>, Child)>;

/// What an insert found under its key: nothing, or a value, with the
/// overflow run it kept, if any, for the caller to give back.
type Replaced = Option<Option<OverflowRef>>;

/// What an insert stores, as the caller gives it or as the leaf stores it,
/// and whether it replaces a value already there, giving it to `visit`
/// first.
struct Put<'a, 'v, V = StoredRef<'a>> {
    key: &'a [u8],
    value: V,
    replace: bool,
    visit: Option<&'v mut Removed<'v>>,
}

/// What is given the value that an insert replaces or a removal takes out,
/// before it goes.
pub(crate) type Removed<'v> = dyn FnMut(&[u8]) -> Result<()> + 'v;

/// What a removal looks for, and how it goes down to it.
struct Take<'k, 'v> {
    key: &'k [u8],
    /// Whether the first committed node on the way is searched for the key
    /// before it is copied.
    probe: bool,
    /// What is given the value before it goes.
    visit: Option<&'v mut Removed<'v>>,
}

/// What an insert did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Inserted {
    /// The key was not there, and now is.
    New,
    /// The key was there, and its value was replaced.
    Replaced,
    /// The key was there, and its value was kept, as the caller asked.
    Kept,
}

/// Stores `value` under `key`, replacing any value already there if
/// `replace` is set, and keeping it otherwise. A kept value leaves the tree
/// as it was, though the nodes on the way to it may have been copied.
pub(crate) fn insert<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
    value: &[u8],
    replace: bool,
) -> Result<Inserted> {
    let put = Put {
        key,
        value,
        replace,
        visit: None,
    };

    insert_one(load, store, tree, root, put)
}

/// [`insert`], replacing any value already there, which it gives to `visit`
/// before it goes, for a caller that needs the value it replaces. An error
/// `visit` returns stores nothing, though the nodes on the way to the key
/// have been copied.
pub(crate) fn insert_with<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
    value: &[u8],
    visit: &mut Removed<'_>,
) -> Result<Inserted> {
    let put = Put {
        key,
        value,
        replace: true,
        visit: Some(visit),
    };

    insert_one(load, store, tree, root, put)
}

fn insert_one<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    put: Put<'_, '_, &[u8]>,
) -> Result<Inserted> {
    let Put {
        key,
        value,
        replace,
        visit,
    } = put;
    let run = store_value(load.page_size(), store, tree, key.len(), value)?;
    let value = run.map_or(StoredRef::Inline(value), StoredRef::Overflow);

    let Some(child) = root.as_mut() else {
        let page = store.allocate()?;
        let mut leaf = Leaf::new(load.page_size());

        if !leaf.insert(0, key, value) {
            return Err(internal("an entry does not fit in an empty leaf"));
        }

        *root = Some(Child::dirty(page, Node::Leaf(leaf)));

        return Ok(Inserted::New);
    };

    let put = Put {
        key,
        value,
        replace,
        visit,
    };
    let (replaced, split) = insert_into(load, store, tree, child, None, put)?;

    if let Some((separator, right)) = split {
        let level = dirty_node(child)?.level() + 1;
        let left = mem::replace(child, Child::Clean(Pointer::NULL));
        let page = store.allocate()?;

        *child = Child::dirty(
            page,
            Node::Branch(Branch {
                level,
                keys: Keys::of([separator.as_slice()]),
                children: vec![left, right],
            }),
        );
    }

    match replaced {
        // The value was kept: the one written for it goes.
        Some(_) if !replace => {
            release_run(store, run);

            Ok(Inserted::Kept)
        }
        Some(old) => {
            release_run(store, old);

            Ok(Inserted::Replaced)
        }
        None => Ok(Inserted::New),
    }
}

fn insert_into<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    child: &mut Child,
    level: Option<u8>,
    put: Put<'_, '_>,
) -> Result<(Replaced, Split)> {
    let Put {
        key,
        value,
        replace,
        visit,
    } = put;
    let page_size = load.page_size();
    let capacity = content_len(page_size);
    let node = make_dirty(load, store, child, tree, level)?;

    match node {
        Node::Leaf(leaf) => {
            let found = leaf.search(key);

            if let (Ok(index), false) = (found, replace) {
                let kept = match leaf.value(index).map_err(internal)? {
                    StoredRef::Inline(_) => None,
                    StoredRef::Overflow(reference) => Some(reference),
                };

                return Ok((Some(kept), None));
            }

            if let (Ok(index), Some(visit)) = (found, visit) {
                let visited = match leaf.value(index).map_err(internal)? {
                    StoredRef::Inline(old) => visit(old),
                    StoredRef::Overflow(reference) => load
                        .read_overflow(&reference, tree)
                        .and_then(|old| visit(&old)),
                };

                if let Err(error) = visited {
                    // Nothing refers to the pages written for the value.
                    if let StoredRef::Overflow(run) = value {
                        release_run(store, Some(run));
                    }

                    return Err(error);
                }
            }

            if let Ok(index) = found {
                if let Some(replaced) = leaf.overwrite(index, key, value).map_err(internal)? {
                    return Ok((Some(replaced), None));
                }
            }

            let replaced = match found {
                Ok(index) => Some(leaf.remove(index).map_err(internal)?),
                Err(_) => None,
            };
            let at = found.unwrap_or_else(|index| index);

            if leaf.insert(at, key, value) {
                return Ok((replaced, None));
            }

            // The page is full: its entries and the new one are shared out
            // between it and a new leaf, as evenly as their sizes allow.
            let mut sizes: Vec<usize> = (0..leaf.len())
                .map(|index| leaf.entry_size(index))
                .collect();

            sizes.insert(at, cell_len(key.len(), value) + 2);

            // An entry after every other, as keys that only grow bring, goes
            // alone into the new leaf: the full one stays as it is, and a run
            // of such inserts fills its leaves rather than leaving each half
            // empty.
            let middle = if at == leaf.len() {
                at
            } else {
                split_point(&sizes, capacity)
                    .ok_or_else(|| internal("a leaf that cannot be split"))?
            };
            let fits = if middle <= at {
                let mut right = leaf.split_off(middle);
                let fits = right.insert(at - middle, key, value);

                (fits, right)
            } else {
                let right = leaf.split_off(middle - 1);

                (leaf.insert(at, key, value), right)
            };
            let (true, right) = fits else {
                return Err(internal("a split leaf's entry does not fit"));
            };
            let separator = right.key(0).to_vec();
            let page = store.allocate()?;

            Ok((
                replaced,
                Some((separator, Child::dirty(page, Node::Leaf(right)))),
            ))
        }
        Node::Branch(branch) => {
            let index = branch.keys.child_index(key);
            let (replaced, split) = insert_into(
                load,
                store,
                tree,
                &mut branch.children[index],
                Some(branch.level - 1),
                Put {
                    key,
                    value,
                    replace,
                    visit,
                },
            )?;
            let mut split_up = None;

            if let Some((separator, right)) = split {
                branch.keys.insert(index, &separator);
                branch.children.insert(index + 1, right);

                if branch.keys.branch_len() > capacity {
                    split_up = Some(split_branch(store, branch, capacity)?);
                }
            }

            Ok((replaced, split_up))
        }
    }
}

/// What an update makes of the value under its key: a new value, or `None`
/// to keep the one there.
pub(crate) type Change<'v> = dyn FnMut(&[u8]) -> Result<Option<Vec<u8>>> + 'v;

/// What an update found under its key.
enum Updated {
    /// Nothing.
    Missing,
    /// A value, which the change kept.
    Kept,
    /// A value, which the change replaced, with the overflow run it had, if
    /// any, for the caller to give back.
    Replaced(Option<OverflowRef>),
}

/// Replaces the value under `key` with what `change` makes of it, going down
/// to the key once, for a caller whose new value depends on the old: reading
/// the value first and then replacing it went down twice. Returns whether
/// the key was there. The nodes on the way to it are copied whether it is
/// there or not, and whether `change` keeps the value or not; an error
/// `change` returns stores nothing.
pub(crate) fn update_with<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
    change: &mut Change<'_>,
) -> Result<bool> {
    let Some(child) = root.as_mut() else {
        return Ok(false);
    };
    let (updated, split) = update_into(load, store, tree, child, None, key, change)?;

    grow_root(store, child, split)?;

    Ok(match updated {
        Updated::Missing => false,
        Updated::Kept => true,
        Updated::Replaced(run) => {
            release_run(store, run);

            true
        }
    })
}

fn update_into<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    child: &mut Child,
    level: Option<u8>,
    key: &[u8],
    change: &mut Change<'_>,
) -> Result<(Updated, Split)> {
    let page_size = load.page_size();
    let capacity = content_len(page_size);
    let node = make_dirty(load, store, child, tree, level)?;

    match node {
        Node::Leaf(leaf) => {
            let Ok(index) = leaf.search(key) else {
                return Ok((Updated::Missing, None));
            };
            let changed = match leaf.value(index).map_err(internal)? {
                StoredRef::Inline(old) => change(old)?,
                StoredRef::Overflow(reference) => change(&load.read_overflow(&reference, tree)?)?,
            };
            let Some(value) = changed else {
                return Ok((Updated::Kept, None));
            };
            let run = store_value(page_size, store, tree, key.len(), &value)?;
            let stored = run.map_or(StoredRef::Inline(&value), StoredRef::Overflow);
            let (replaced, split) = replace_in_leaf(store, leaf, index, key, stored, capacity)?;

            Ok((Updated::Replaced(replaced), split))
        }
        Node::Branch(branch) => {
            let index = branch.keys.child_index(key);
            let (updated, split) = update_into(
                load,
                store,
                tree,
                &mut branch.children[index],
                Some(branch.level - 1),
                key,
                change,
            )?;

            Ok((updated, take_split(store, branch, index, split, capacity)?))
        }
    }
}

/// Puts a new root above `root` when the root split in two, as
/// [`insert_one`] does.
fn grow_root<S: Store>(store: &mut S, root: &mut Child, split: Split) -> Result<()> {
    let Some((separator, right)) = split else {
        return Ok(());
    };
    let level = dirty_node(root)?.level() + 1;
    let left = mem::replace(root, Child::Clean(Pointer::NULL));
    let page = store.allocate()?;

    *root = Child::dirty(
        page,
        Node::Branch(Branch {
            level,
            keys: Keys::of([separator.as_slice()]),
            children: vec![left, right],
        }),
    );

    Ok(())
}

/// Replaces the value of entry `index` of `leaf`, whose key is `key`, with
/// `value`: in place where it fits, or else by taking the entry out and
/// putting it back, splitting the leaf when it does not fit. Returns the
/// overflow run the old value had, if any, and the split.
///
/// This is what [`insert_into`] does with a key it finds, and it keeps its
/// own copy: calling a function shared with it from there moved the code of
/// the insert path and made inserts 1.5% and deletes 4% slower, measured on
/// separately built binaries, with no other change.
fn replace_in_leaf<S: Store>(
    store: &mut S,
    leaf: &mut Leaf,
    index: usize,
    key: &[u8],
    value: StoredRef<'_>,
    capacity: usize,
) -> Result<(Option<OverflowRef>, Split)> {
    if let Some(replaced) = leaf.overwrite(index, key, value).map_err(internal)? {
        return Ok((replaced, None));
    }

    let replaced = leaf.remove(index).map_err(internal)?;

    if leaf.insert(index, key, value) {
        return Ok((replaced, None));
    }

    // The page is full: its entries and the new one are shared out between
    // it and a new leaf, as evenly as their sizes allow.
    let mut sizes: Vec<usize> = (0..leaf.len()).map(|at| leaf.entry_size(at)).collect();

    sizes.insert(index, cell_len(key.len(), value) + 2);

    let middle =
        split_point(&sizes, capacity).ok_or_else(|| internal("a leaf that cannot be split"))?;
    let fits = if middle <= index {
        let mut right = leaf.split_off(middle);
        let fits = right.insert(index - middle, key, value);

        (fits, right)
    } else {
        let right = leaf.split_off(middle - 1);

        (leaf.insert(index, key, value), right)
    };
    let (true, right) = fits else {
        return Err(internal("a split leaf's entry does not fit"));
    };
    let separator = right.key(0).to_vec();
    let page = store.allocate()?;

    Ok((
        replaced,
        Some((separator, Child::dirty(page, Node::Leaf(right)))),
    ))
}

/// Takes the split of child `index` of `branch` into the branch, and returns
/// the branch's own split if it no longer fits, as [`insert_into`] does.
fn take_split<S: Store>(
    store: &mut S,
    branch: &mut Branch,
    index: usize,
    split: Split,
    capacity: usize,
) -> Result<Split> {
    let Some((separator, right)) = split else {
        return Ok(None);
    };

    branch.keys.insert(index, &separator);
    branch.children.insert(index + 1, right);

    if branch.keys.branch_len() > capacity {
        return Ok(Some(split_branch(store, branch, capacity)?));
    }

    Ok(None)
}

/// Removes `key` and its value. Returns whether it was there. Nothing is
/// copied when it was not.
pub(crate) fn remove<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
) -> Result<bool> {
    if !contains(load, tree, root.as_ref(), key)? {
        return Ok(false);
    }

    remove_present(load, store, tree, root, key)
}

/// [`remove`] for a key the caller knows to be there, as one it has just
/// read: the removal goes down to it once, rather than looking it up first.
/// The nodes on the way to it are copied whether it is there or not.
pub(crate) fn remove_present<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
) -> Result<bool> {
    let take = Take {
        key,
        probe: false,
        visit: None,
    };

    remove_one(load, store, tree, root, take)
}

/// [`remove`], giving `visit` the value before it goes, for a caller that
/// needs the value of what it removes. The part of the path to the key that
/// the transaction has copied already is gone down once rather than twice:
/// the first committed node on the way is searched before it is copied, and
/// nothing is copied when the key is not there. An error `visit` returns
/// removes nothing, though the nodes on the way to the key have been copied.
pub(crate) fn remove_with<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
    visit: &mut Removed<'_>,
) -> Result<bool> {
    let take = Take {
        key,
        probe: true,
        visit: Some(visit),
    };

    remove_one(load, store, tree, root, take)
}

fn remove_one<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    take: Take<'_, '_>,
) -> Result<bool> {
    let Some(child) = root.as_mut() else {
        return Ok(false);
    };
    let removed = remove_from(load, store, tree, child, None, take)?;

    if let Some(run) = removed {
        release_run(store, run);
    }

    collapse_root(store, root)?;

    Ok(removed.is_some())
}

fn remove_from<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    child: &mut Child,
    level: Option<u8>,
    take: Take<'_, '_>,
) -> Result<Replaced> {
    let Take {
        key,
        mut probe,
        visit,
    } = take;

    // Every node below a committed one is committed too, so once the key is
    // known to be under it, nothing further down needs searching first.
    if probe && matches!(child, Child::Clean(_)) {
        if !contains_below(load, tree, child, level, key)? {
            return Ok(None);
        }

        probe = false;
    }

    let node = make_dirty(load, store, child, tree, level)?;

    match node {
        Node::Leaf(leaf) => match leaf.search(key) {
            Ok(index) => {
                if let Some(visit) = visit {
                    match leaf.value(index).map_err(internal)? {
                        StoredRef::Inline(value) => visit(value)?,
                        StoredRef::Overflow(reference) => {
                            visit(&load.read_overflow(&reference, tree)?)?;
                        }
                    }
                }

                Ok(Some(leaf.remove(index).map_err(internal)?))
            }
            Err(_) => Ok(None),
        },
        Node::Branch(branch) => {
            let index = branch.keys.child_index(key);
            let removed = remove_from(
                load,
                store,
                tree,
                &mut branch.children[index],
                Some(branch.level - 1),
                Take { key, probe, visit },
            )?;

            // A child nothing was removed from is the size it was.
            if removed.is_some() {
                rebalance(load, store, tree, branch, index)?;
            }

            Ok(removed)
        }
    }
}

/// An empty root leaf empties the tree; a root branch left with one child
/// hands the root to that child.
fn collapse_root<S: Store>(store: &mut S, root: &mut Option<Child>) -> Result<()> {
    loop {
        let Some(Child::Dirty { page, node }) = root.as_mut() else {
            return Ok(());
        };
        let page = *page;

        match node.as_mut() {
            Node::Leaf(leaf) if leaf.is_empty() => {
                store.release(page, store.txn());
                *root = None;

                return Ok(());
            }
            Node::Branch(branch) if branch.keys.is_empty() => {
                let only = branch
                    .children
                    .pop()
                    .ok_or_else(|| internal("a branch without children"))?;

                store.release(page, store.txn());
                *root = Some(only);
            }
            _ => return Ok(()),
        }
    }
}

/// Repairs child `index` of `branch` after a removal left it small.
fn rebalance<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    branch: &mut Branch,
    index: usize,
) -> Result<()> {
    let capacity = content_len(load.page_size());
    let child_level = Some(branch.level - 1);
    let (empty_leaf, underfull) = {
        let node = dirty_node(&branch.children[index])?;
        let empty = match node {
            Node::Leaf(leaf) => leaf.is_empty(),
            Node::Branch(child) => child.keys.is_empty(),
        };

        (
            empty && matches!(node, Node::Leaf(_)),
            empty || node.len() < capacity / 4,
        )
    };

    if !underfull || branch.children.len() < 2 {
        return Ok(());
    }

    if empty_leaf {
        let removed = branch.children.remove(index);

        branch.keys.remove(index.saturating_sub(1));
        release_child(store, &removed);

        return Ok(());
    }

    let left = if index > 0 { index - 1 } else { index };
    let right = left + 1;
    let children_are_leaves = branch.level == 1;
    let separator = branch.keys.get(left).to_vec();
    let (before, after) = branch.children.split_at_mut(right);
    let (left_child, right_child) = (&mut before[left], &mut after[0]);
    let left_len = super::resolve(load, left_child, tree, child_level)?.size();
    let right_len = super::resolve(load, right_child, tree, child_level)?.size();

    // Both neighbours' content goes into the left one. If it fits, the right
    // one is gone; otherwise the combined node is split again, evenly.
    let (merged, split) = if children_are_leaves {
        let Node::Leaf(moved) = take_node(load, store, right_child, tree, child_level)? else {
            return Err(internal("a leaf's neighbour is a branch"));
        };
        let Node::Leaf(kept) = make_dirty(load, store, left_child, tree, child_level)? else {
            return Err(internal("a leaf's neighbour is a branch"));
        };
        let mut entries = kept.to_entries().map_err(internal)?;

        entries.extend(moved.to_entries().map_err(internal)?);

        if left_len + right_len <= capacity {
            *kept = Leaf::from_entries(load.page_size(), &entries);

            (true, None)
        } else {
            let (left, separator, right) = split_leaf(entries, load.page_size())?;
            let page = store.allocate()?;

            *kept = left;

            (
                false,
                Some((separator, Child::dirty(page, Node::Leaf(right)))),
            )
        }
    } else {
        let Node::Branch(moved) = take_node(load, store, right_child, tree, child_level)? else {
            return Err(internal("a branch's neighbour is a leaf"));
        };
        let Node::Branch(kept) = make_dirty(load, store, left_child, tree, child_level)? else {
            return Err(internal("a branch's neighbour is a leaf"));
        };

        kept.keys.push(&separator);
        kept.keys.append(&moved.keys);
        kept.children.extend(moved.children);

        if kept.keys.branch_len() <= capacity {
            (true, None)
        } else {
            (false, Some(split_branch(store, kept, capacity)?))
        }
    };

    if let Some((separator, new_right)) = split {
        *right_child = new_right;
        branch.keys.set(left, &separator);
    }

    if merged {
        branch.keys.remove(left);
        branch.children.remove(right);
    }

    Ok(())
}

/// Makes `child` a page of this transaction, copying the committed node it
/// refers to, and returns the node to change.
fn make_dirty<'c, L: Load, S: Store>(
    load: &L,
    store: &mut S,
    child: &'c mut Child,
    tree: u64,
    level: Option<u8>,
) -> Result<&'c mut Node> {
    if let Child::Clean(pointer) = child {
        let pointer = *pointer;
        let node = load.node_to_change(store, &pointer, tree, level)?;
        let page = store.allocate()?;

        store.release(pointer.page, pointer.txn);
        *child = Child::dirty(page, node);
    }

    match child {
        Child::Dirty { node, .. } => Ok(node),
        Child::Clean(_) => Err(internal("a page stayed clean after being copied")),
    }
}

/// Takes the node out of `child`, giving its page back.
fn take_node<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    child: &mut Child,
    tree: u64,
    level: Option<u8>,
) -> Result<Node> {
    match mem::replace(child, Child::Clean(Pointer::NULL)) {
        Child::Clean(pointer) => {
            let node = load.node_to_change(store, &pointer, tree, level)?;

            store.release(pointer.page, pointer.txn);

            Ok(node)
        }
        Child::Dirty { page, node } => {
            store.release(page, store.txn());

            Ok(*node)
        }
    }
}

/// The node of a child this transaction holds.
fn dirty_node(child: &Child) -> Result<&Node> {
    match child {
        Child::Dirty { node, .. } => Ok(node),
        Child::Clean(_) => Err(internal("expected a page of this transaction")),
    }
}

fn release_child<S: Store>(store: &mut S, child: &Child) {
    match child {
        Child::Clean(pointer) => store.release(pointer.page, pointer.txn),
        Child::Dirty { page, .. } => store.release(*page, store.txn()),
    }
}

/// Where to cut a sequence of entries of `sizes` bytes so that both parts
/// fit in `capacity` and differ in size as little as they can: the index of
/// the first entry of the second part.
fn split_point(sizes: &[usize], capacity: usize) -> Option<usize> {
    let total: usize = sizes.iter().sum();
    let mut best: Option<(usize, usize)> = None;
    let mut left = 0;

    for at in 1..sizes.len() {
        left += sizes[at - 1];

        if left <= capacity && total - left <= capacity {
            let distance = left.abs_diff(total - left);

            if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                best = Some((at, distance));
            }
        }
    }

    best.map(|(at, _)| at)
}

/// Shares out the entries of an overflowing leaf, in order, between two
/// leaves as evenly as their sizes allow. Returns the left leaf, the
/// separator, which is the first key of the right one, and the right leaf.
fn split_leaf(mut entries: Vec<LeafEntry>, page_size: usize) -> Result<(Leaf, Vec<u8>, Leaf)> {
    let sizes: Vec<usize> = entries.iter().map(LeafEntry::len).collect();
    let at = split_point(&sizes, content_len(page_size))
        .ok_or_else(|| internal("a leaf that cannot be split"))?;
    let right = entries.split_off(at);
    let separator = right[0].key.clone();

    Ok((
        Leaf::from_entries(page_size, &entries),
        separator,
        Leaf::from_entries(page_size, &right),
    ))
}

/// Splits an overflowing branch, keeping the left part in place. The key in
/// the middle moves up to the parent.
fn split_branch<S: Store>(
    store: &mut S,
    branch: &mut Branch,
    capacity: usize,
) -> Result<(Vec<u8>, Child)> {
    let sizes: Vec<usize> = branch
        .keys
        .iter()
        .map(|key| branch_key_len(key.len()))
        .collect();
    let total: usize = sizes.iter().sum();
    let mut best = None;
    let mut before = 0;

    // Key `middle` goes up; each side keeps at least one key.
    for middle in 1..sizes.len().saturating_sub(1) {
        before += sizes[middle - 1];

        let left = POINTER_LEN + before;
        let right = POINTER_LEN + total - before - sizes[middle];

        if left <= capacity && right <= capacity {
            let distance = left.abs_diff(right);

            if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                best = Some((middle, distance));
            }
        }
    }

    let (middle, _) = best.ok_or_else(|| internal("a branch that cannot be split"))?;
    let right_keys = branch.keys.split_off(middle + 1);
    let separator = branch
        .keys
        .pop()
        .ok_or_else(|| internal("a branch split without a middle key"))?;
    let right_children = branch.children.split_off(middle + 1);
    let page = store.allocate()?;

    Ok((
        separator,
        Child::dirty(
            page,
            Node::Branch(Branch {
                level: branch.level,
                keys: right_keys,
                children: right_children,
            }),
        ),
    ))
}

/// Keeps a small value in the leaf, and writes a large one to an overflow
/// run, whose reference it returns.
// Inlined by hand, as the functions an insert calls that an update calls
// too are: with a second caller the compiler stopped inlining them into the
// insert path, and inserts through the Node.js package took 1.9% longer.
#[inline(always)]
fn store_value<S: Store>(
    page_size: usize,
    store: &mut S,
    tree: u64,
    key_len: usize,
    value: &[u8],
) -> Result<Option<OverflowRef>> {
    if inline_entry_len(key_len, value.len()) <= inline_limit(page_size) {
        return Ok(None);
    }

    let len = value.len() as u64;
    let pages = overflow_pages(page_size, len);
    let first = store.allocate_run(pages)?;
    let chunk = content_len(page_size);
    let mut checks = Vec::with_capacity(16 * value.len().div_ceil(chunk));
    let mut index = 0u64;

    // A run of pages at a time, each written with one call.
    for parts in value.chunks(chunk * store.run_pages()) {
        let run_first = first + index;
        let mut bytes = vec![0u8; parts.len().div_ceil(chunk) * page_size];

        for (part, page) in parts.chunks(chunk).zip(bytes.chunks_mut(page_size)) {
            PageHeader {
                kind: PageKind::Overflow,
                level: 0,
                count: 0,
                txn: store.txn(),
                tree,
                index,
            }
            .write(page);
            page[CONTENT_OFFSET..CONTENT_OFFSET + part.len()].copy_from_slice(part);
            index += 1;
        }

        for check in store.write_run(run_first, &mut bytes)? {
            checks.extend_from_slice(&check.0);
        }
    }

    Ok(Some(OverflowRef {
        first,
        txn: store.txn(),
        pages: u32::try_from(pages).map_err(|_| internal("an overflow run too long"))?,
        len,
        check: Check::of(&[&checks]),
    }))
}

/// Gives back the overflow pages of a value that is no longer stored.
fn release_run<S: Store>(store: &mut S, run: Option<OverflowRef>) {
    if let Some(reference) = run {
        for index in 0..u64::from(reference.pages) {
            store.release(reference.first + index, reference.txn);
        }
    }
}

/// Gives back every page of the tree rooted at `root`, overflow runs included.
pub(crate) fn delete_tree<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: Option<Child>,
) -> Result<()> {
    let Some(root) = root else {
        return Ok(());
    };
    let mut pending = vec![(root, None)];

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
                for index in 0..leaf.len() {
                    if let StoredRef::Overflow(reference) = leaf.value(index).map_err(internal)? {
                        release_run(store, Some(reference));
                    }
                }
            }
            Node::Branch(branch) => {
                let level = Some(branch.level - 1);

                pending.extend(branch.children.into_iter().map(|child| (child, level)));
            }
        }
    }

    Ok(())
}
