//! A B+tree node in memory, and the reference a branch or a root keeps to it.

use std::cmp::Ordering;
use std::sync::Arc;

use super::leaf::Leaf;
use crate::format::{
    PageHeader, PageKind, Pointer, StoredRef, branch_child, branch_key, branch_len, branch_size,
    check_branch, check_leaf, decode_branch, leaf_entry, leaf_inline, leaf_key, leaf_size,
    leaf_value,
};

/// A child of a branch, or the root of a tree.
///
/// A committed page is referred to by its pointer. A page the current write
/// transaction has written is held in memory, in place, until the commit
/// writes it: the part of a tree a transaction changed is an ordinary owned
/// tree hanging off the committed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Child {
    /// A committed page.
    Clean(Pointer),
    /// A page this transaction allocated, with the node it will hold.
    Dirty { page: u64, node: Box<Node> },
}

impl Child {
    /// A new page holding `node`.
    pub(crate) fn dirty(page: u64, node: Node) -> Self {
        Child::Dirty {
            page,
            node: Box::new(node),
        }
    }
}

/// A B+tree node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    /// Entries in ascending key order, laid out as their page.
    Leaf(Leaf),
    /// Keys and one more child than keys.
    Branch(Branch),
}

/// The inside of a branch node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Branch {
    /// One more than the level of the children.
    pub(crate) level: u8,
    /// The separator keys, in ascending order.
    pub(crate) keys: Vec<Vec<u8>>,
    /// The children: child `i` holds the keys from `keys[i − 1]` up to, but
    /// not including, `keys[i]`.
    pub(crate) children: Vec<Child>,
}

impl Node {
    /// 0 for a leaf; one more than the children for a branch.
    pub(crate) fn level(&self) -> u8 {
        match self {
            Node::Leaf(_) => 0,
            Node::Branch(branch) => branch.level,
        }
    }

    /// The bytes of a page's content area this node takes.
    pub(crate) fn len(&self) -> usize {
        match self {
            Node::Leaf(leaf) => leaf.size(),
            Node::Branch(branch) => branch_len(&branch.keys),
        }
    }
}

/// The child that holds `key`: the number of separators at or below it.
pub(crate) fn child_index(keys: &[Vec<u8>], key: &[u8]) -> usize {
    keys.partition_point(|separator| separator.as_slice() <= key)
}

/// A committed node as the page cache keeps it: its page, checked once and
/// read in place, and the tree and commit its page header named, which every
/// later reader checks again.
///
/// Keeping the page rather than a [`Node`] costs one allocation where a node
/// costs one per key and value, both when a page is read and when the cache
/// lets it go, and it keeps the keys a search reads close together. A write
/// transaction that changes the node decodes it into a [`Node`] of its own.
#[derive(Debug)]
pub(crate) struct LoadedNode {
    pub(crate) tree: u64,
    pub(crate) txn: u64,
    level: u8,
    leaf: bool,
    /// A leaf's entries, or a branch's keys.
    count: usize,
    /// The bytes of the page's content the node takes, as [`Node::len`]
    /// counts them.
    size: usize,
    page: Box<[u8]>,
}

impl LoadedNode {
    /// The node on the verified page `page`, whose header is `header`, once
    /// every offset and length in it checks out.
    pub(crate) fn read(page: Vec<u8>, header: &PageHeader) -> Result<Self, &'static str> {
        let count = usize::from(header.count);

        match header.kind {
            PageKind::Leaf => check_leaf(&page, count)?,
            PageKind::Branch => check_branch(&page, count)?,
            PageKind::Overflow => return Err("a tree points at an overflow page"),
        }

        Ok(Self::checked(page, header))
    }

    /// The node on a page this process encoded, before sealing it, which
    /// needs no check.
    pub(crate) fn encoded(page: Vec<u8>, header: &PageHeader) -> Self {
        debug_assert!(
            match header.kind {
                PageKind::Leaf => check_leaf(&page, usize::from(header.count)),
                _ => check_branch(&page, usize::from(header.count)),
            }
            .is_ok()
        );

        Self::checked(page, header)
    }

    fn checked(page: Vec<u8>, header: &PageHeader) -> Self {
        let count = usize::from(header.count);
        let leaf = header.kind == PageKind::Leaf;
        let size = if leaf {
            leaf_size(&page, count)
        } else {
            branch_size(&page, count)
        };

        Self {
            tree: header.tree,
            txn: header.txn,
            level: header.level,
            leaf,
            count,
            size,
            page: page.into_boxed_slice(),
        }
    }

    /// 0 for a leaf; one more than the children for a branch.
    pub(crate) fn level(&self) -> u8 {
        self.level
    }

    /// The node, decoded for a write transaction to change.
    pub(crate) fn to_node(&self) -> crate::error::Result<Node> {
        let node = if self.leaf {
            Ok(Node::Leaf(Leaf::from_page(&self.page, self.count)))
        } else {
            decode_branch(&self.page, self.count).map(|(keys, children)| {
                Node::Branch(Branch {
                    level: self.level,
                    keys,
                    children: children.into_iter().map(Child::Clean).collect(),
                })
            })
        };

        node.map_err(super::read::internal)
    }

    /// Child `index` of a branch.
    pub(crate) fn child(&self, index: usize) -> Pointer {
        branch_child(&self.page, index)
    }
}

/// A node that is either borrowed from a write transaction's changes or
/// loaded from a committed page.
#[derive(Debug)]
pub(crate) enum NodeRef<'a> {
    Borrowed(&'a Node),
    Loaded(Arc<LoadedNode>),
}

impl NodeRef<'_> {
    pub(crate) fn is_leaf(&self) -> bool {
        match self {
            NodeRef::Borrowed(node) => matches!(node, Node::Leaf(_)),
            NodeRef::Loaded(loaded) => loaded.leaf,
        }
    }

    /// A leaf's entries, or a branch's keys, which is one fewer than its
    /// children.
    pub(crate) fn count(&self) -> usize {
        match self {
            NodeRef::Borrowed(Node::Leaf(leaf)) => leaf.len(),
            NodeRef::Borrowed(Node::Branch(branch)) => branch.keys.len(),
            NodeRef::Loaded(loaded) => loaded.count,
        }
    }

    /// The bytes of a page's content the node takes.
    pub(crate) fn size(&self) -> usize {
        match self {
            NodeRef::Borrowed(node) => node.len(),
            NodeRef::Loaded(loaded) => loaded.size,
        }
    }

    /// Key `index`: a leaf entry's, or a branch's separator.
    pub(crate) fn key(&self, index: usize) -> &[u8] {
        match self {
            NodeRef::Borrowed(Node::Leaf(leaf)) => leaf.key(index),
            NodeRef::Borrowed(Node::Branch(branch)) => &branch.keys[index],
            NodeRef::Loaded(loaded) if loaded.leaf => leaf_key(&loaded.page, index),
            NodeRef::Loaded(loaded) => branch_key(&loaded.page, loaded.count, index),
        }
    }

    /// The value of a leaf's entry `index`.
    pub(crate) fn value(&self, index: usize) -> crate::error::Result<StoredRef<'_>> {
        match self {
            NodeRef::Borrowed(Node::Leaf(leaf)) => leaf.value(index).map_err(super::read::internal),
            NodeRef::Loaded(loaded) if loaded.leaf => {
                leaf_value(&loaded.page, index).map_err(super::read::internal)
            }
            _ => Err(super::read::internal("read a value of a branch")),
        }
    }

    /// The key and the value of a leaf's entry `index`, if the value is
    /// inline.
    pub(crate) fn inline_entry(&self, index: usize) -> Option<(&[u8], &[u8])> {
        match self {
            NodeRef::Borrowed(Node::Leaf(leaf)) => leaf.inline(index),
            NodeRef::Loaded(loaded) if loaded.leaf => leaf_inline(&loaded.page, index),
            _ => None,
        }
    }

    /// The key and the value of a leaf's entry `index`.
    pub(crate) fn entry(&self, index: usize) -> crate::error::Result<(&[u8], StoredRef<'_>)> {
        match self {
            NodeRef::Borrowed(Node::Leaf(leaf)) => leaf.entry(index).map_err(super::read::internal),
            NodeRef::Loaded(loaded) if loaded.leaf => {
                leaf_entry(&loaded.page, index).map_err(super::read::internal)
            }
            _ => Err(super::read::internal("read an entry of a branch")),
        }
    }

    /// The number of keys of the node below `key`, or at or below it with
    /// `or_equal`: for a branch, the child that holds `key`.
    pub(crate) fn rank(&self, key: &[u8], or_equal: bool) -> usize {
        let (mut low, mut high) = (0, self.count());

        while low < high {
            let middle = low + (high - low) / 2;
            let below = match self.key(middle).cmp(key) {
                Ordering::Less => true,
                Ordering::Equal => or_equal,
                Ordering::Greater => false,
            };

            if below {
                low = middle + 1;
            } else {
                high = middle;
            }
        }

        low
    }

    /// The node, decoded, for a check that reads all of it.
    #[cfg(test)]
    pub(crate) fn to_node(&self) -> crate::error::Result<Node> {
        match self {
            NodeRef::Borrowed(node) => Ok((*node).clone()),
            NodeRef::Loaded(loaded) => loaded.to_node(),
        }
    }
}

impl Clone for NodeRef<'_> {
    fn clone(&self) -> Self {
        match self {
            NodeRef::Borrowed(node) => NodeRef::Borrowed(node),
            NodeRef::Loaded(node) => NodeRef::Loaded(Arc::clone(node)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_goes_to_the_child_whose_range_holds_it() {
        let keys = vec![b"g".to_vec(), b"p".to_vec()];

        assert_eq!(child_index(&keys, b"a"), 0);
        assert_eq!(child_index(&keys, b"g"), 1, "a separator starts its child");
        assert_eq!(child_index(&keys, b"o"), 1);
        assert_eq!(child_index(&keys, b"p"), 2);
        assert_eq!(child_index(&keys, b"z"), 2);
    }
}
