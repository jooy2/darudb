//! Changing a tree, copy-on-write.
//!
//! The first change to a committed node copies it into a page of the
//! transaction ([`make_dirty`]); later changes in the same transaction change
//! that copy in place. A node that grows past its page is split in two. A node
//! that shrinks below a quarter of its page is merged with a neighbour, or,
//! when the two do not fit in one page, the entries of both are shared out
//! again evenly.

use std::mem;

use super::node::{Branch, Child, Node, child_index};
use super::read::{get_stored, internal};
use super::{Load, Store};
use crate::error::Result;
use crate::format::{
    CONTENT_OFFSET, Check, LeafEntry, OverflowRef, POINTER_LEN, PageHeader, PageKind, Pointer,
    StoredValue, branch_key_len, branch_len, content_len, inline_entry_len, inline_limit,
    overflow_pages,
};

/// A node split in two: the separator and the new right-hand node.
type Split = Option<(Vec<u8>, Child)>;

/// Stores `value` under `key`, replacing any value already there. Returns
/// whether one was.
pub(crate) fn insert<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    root: &mut Option<Child>,
    key: &[u8],
    value: &[u8],
) -> Result<bool> {
    let value = store_value(load.page_size(), store, tree, key.len(), value)?;

    let Some(child) = root.as_mut() else {
        let page = store.allocate()?;

        *root = Some(Child::dirty(
            page,
            Node::Leaf(vec![LeafEntry {
                key: key.to_vec(),
                value,
            }]),
        ));

        return Ok(false);
    };

    let (replaced, split) = insert_into(load, store, tree, child, None, key, value)?;

    if let Some((separator, right)) = split {
        let level = dirty_node(child)?.level() + 1;
        let left = mem::replace(child, Child::Clean(Pointer::NULL));
        let page = store.allocate()?;

        *child = Child::dirty(
            page,
            Node::Branch(Branch {
                level,
                keys: vec![separator],
                children: vec![left, right],
            }),
        );
    }

    match replaced {
        Some(old) => {
            release_value(store, &old);

            Ok(true)
        }
        None => Ok(false),
    }
}

fn insert_into<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    child: &mut Child,
    level: Option<u8>,
    key: &[u8],
    value: StoredValue,
) -> Result<(Option<StoredValue>, Split)> {
    let capacity = content_len(load.page_size());
    let node = make_dirty(load, store, child, tree, level)?;

    match node {
        Node::Leaf(entries) => {
            let replaced = match entries.binary_search_by(|entry| entry.key.as_slice().cmp(key)) {
                Ok(index) => Some(mem::replace(&mut entries[index].value, value)),
                Err(index) => {
                    entries.insert(
                        index,
                        LeafEntry {
                            key: key.to_vec(),
                            value,
                        },
                    );

                    None
                }
            };
            let split = if leaf_len(entries) > capacity {
                Some(split_leaf(store, entries, capacity)?)
            } else {
                None
            };

            Ok((replaced, split))
        }
        Node::Branch(branch) => {
            let index = child_index(&branch.keys, key);
            let (replaced, split) = insert_into(
                load,
                store,
                tree,
                &mut branch.children[index],
                Some(branch.level - 1),
                key,
                value,
            )?;
            let mut split_up = None;

            if let Some((separator, right)) = split {
                branch.keys.insert(index, separator);
                branch.children.insert(index + 1, right);

                if branch_len(&branch.keys) > capacity {
                    split_up = Some(split_branch(store, branch, capacity)?);
                }
            }

            Ok((replaced, split_up))
        }
    }
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
    if get_stored(load, tree, root.as_ref(), key)?.is_none() {
        return Ok(false);
    }

    let Some(child) = root.as_mut() else {
        return Ok(false);
    };

    if let Some(value) = remove_from(load, store, tree, child, None, key)? {
        release_value(store, &value);
    }

    collapse_root(store, root)?;

    Ok(true)
}

fn remove_from<L: Load, S: Store>(
    load: &L,
    store: &mut S,
    tree: u64,
    child: &mut Child,
    level: Option<u8>,
    key: &[u8],
) -> Result<Option<StoredValue>> {
    let node = make_dirty(load, store, child, tree, level)?;

    match node {
        Node::Leaf(entries) => Ok(entries
            .binary_search_by(|entry| entry.key.as_slice().cmp(key))
            .ok()
            .map(|index| entries.remove(index).value)),
        Node::Branch(branch) => {
            let index = child_index(&branch.keys, key);
            let removed = remove_from(
                load,
                store,
                tree,
                &mut branch.children[index],
                Some(branch.level - 1),
                key,
            )?;

            rebalance(load, store, tree, branch, index)?;

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
            Node::Leaf(entries) if entries.is_empty() => {
                store.release(page);
                *root = None;

                return Ok(());
            }
            Node::Branch(branch) if branch.keys.is_empty() => {
                let only = branch
                    .children
                    .pop()
                    .ok_or_else(|| internal("a branch without children"))?;

                store.release(page);
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
            Node::Leaf(entries) => entries.is_empty(),
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
    let separator = branch.keys[left].clone();
    let (before, after) = branch.children.split_at_mut(right);
    let (left_child, right_child) = (&mut before[left], &mut after[0]);
    let left_len = super::resolve(load, left_child, tree, child_level)?.len();
    let right_len = super::resolve(load, right_child, tree, child_level)?.len();

    // Both neighbours' content goes into the left one. If it fits, the right
    // one is gone; otherwise the combined node is split again, evenly.
    let (merged, split) = if children_are_leaves {
        let Node::Leaf(moved) = take_node(load, store, right_child, tree, child_level)? else {
            return Err(internal("a leaf's neighbour is a branch"));
        };
        let Node::Leaf(entries) = make_dirty(load, store, left_child, tree, child_level)? else {
            return Err(internal("a leaf's neighbour is a branch"));
        };

        entries.extend(moved);

        if left_len + right_len <= capacity {
            (true, None)
        } else {
            (false, Some(split_leaf(store, entries, capacity)?))
        }
    } else {
        let Node::Branch(moved) = take_node(load, store, right_child, tree, child_level)? else {
            return Err(internal("a branch's neighbour is a leaf"));
        };
        let Node::Branch(kept) = make_dirty(load, store, left_child, tree, child_level)? else {
            return Err(internal("a branch's neighbour is a leaf"));
        };

        kept.keys.push(separator);
        kept.keys.extend(moved.keys);
        kept.children.extend(moved.children);

        if branch_len(&kept.keys) <= capacity {
            (true, None)
        } else {
            (false, Some(split_branch(store, kept, capacity)?))
        }
    };

    if let Some((separator, new_right)) = split {
        *right_child = new_right;
        branch.keys[left] = separator;
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
        let loaded = load.load(&pointer, tree, level)?;
        let page = store.allocate()?;

        store.release(pointer.page);
        *child = Child::dirty(page, loaded.node.clone());
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
            let loaded = load.load(&pointer, tree, level)?;

            store.release(pointer.page);

            Ok(loaded.node.clone())
        }
        Child::Dirty { page, node } => {
            store.release(page);

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
        Child::Clean(pointer) => store.release(pointer.page),
        Child::Dirty { page, .. } => store.release(*page),
    }
}

fn leaf_len(entries: &[LeafEntry]) -> usize {
    entries.iter().map(LeafEntry::len).sum()
}

/// Splits an overflowing leaf, keeping the left half in place.
fn split_leaf<S: Store>(
    store: &mut S,
    entries: &mut Vec<LeafEntry>,
    capacity: usize,
) -> Result<(Vec<u8>, Child)> {
    let sizes: Vec<usize> = entries.iter().map(LeafEntry::len).collect();
    let total: usize = sizes.iter().sum();
    let mut best = None;
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

    let (at, _) = best.ok_or_else(|| internal("a leaf that cannot be split"))?;
    let right = entries.split_off(at);
    let separator = right[0].key.clone();
    let page = store.allocate()?;

    Ok((separator, Child::dirty(page, Node::Leaf(right))))
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

/// Keeps a small value in the leaf, and writes a large one to an overflow run.
fn store_value<S: Store>(
    page_size: usize,
    store: &mut S,
    tree: u64,
    key_len: usize,
    value: &[u8],
) -> Result<StoredValue> {
    if inline_entry_len(key_len, value.len()) <= inline_limit(page_size) {
        return Ok(StoredValue::Inline(value.to_vec()));
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

    Ok(StoredValue::Overflow(OverflowRef {
        first,
        txn: store.txn(),
        pages: u32::try_from(pages).map_err(|_| internal("an overflow run too long"))?,
        len,
        check: Check::of(&[&checks]),
    }))
}

/// Gives back the overflow pages of a value that is no longer stored.
fn release_value<S: Store>(store: &mut S, value: &StoredValue) {
    if let StoredValue::Overflow(reference) = value {
        for index in 0..u64::from(reference.pages) {
            store.release(reference.first + index);
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
                let loaded = load.load(&pointer, tree, level)?;

                store.release(pointer.page);
                loaded.node.clone()
            }
            Child::Dirty { page, node } => {
                store.release(page);
                *node
            }
        };

        match node {
            Node::Leaf(entries) => {
                for entry in &entries {
                    release_value(store, &entry.value);
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
