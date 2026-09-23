//! A B+tree node in memory, and the reference a branch or a root keeps to it.

use std::ops::Deref;
use std::sync::Arc;

use crate::format::{
    LeafEntry, PageHeader, PageKind, Pointer, branch_len, decode_branch, decode_leaf,
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
    /// Entries in ascending key order.
    Leaf(Vec<LeafEntry>),
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
            Node::Leaf(entries) => entries.iter().map(LeafEntry::len).sum(),
            Node::Branch(branch) => branch_len(&branch.keys),
        }
    }

    /// Decodes a verified page whose header has been read.
    pub(crate) fn decode(page: &[u8], header: &PageHeader) -> Result<Node, &'static str> {
        let count = usize::from(header.count);

        match header.kind {
            PageKind::Leaf => Ok(Node::Leaf(decode_leaf(page, count)?)),
            PageKind::Branch => {
                let (keys, children) = decode_branch(page, count)?;

                Ok(Node::Branch(Branch {
                    level: header.level,
                    keys,
                    children: children.into_iter().map(Child::Clean).collect(),
                }))
            }
            PageKind::Overflow => Err("a tree points at an overflow page"),
        }
    }
}

/// The child that holds `key`: the number of separators at or below it.
pub(crate) fn child_index(keys: &[Vec<u8>], key: &[u8]) -> usize {
    keys.partition_point(|separator| separator.as_slice() <= key)
}

/// A committed node as the page cache keeps it: the node, and the tree and
/// commit its page header named, which every later reader checks again.
#[derive(Debug)]
pub(crate) struct LoadedNode {
    pub(crate) tree: u64,
    pub(crate) txn: u64,
    pub(crate) node: Node,
}

/// A node that is either borrowed from a write transaction's changes or
/// loaded from a committed page.
#[derive(Debug)]
pub(crate) enum NodeRef<'a> {
    Borrowed(&'a Node),
    Loaded(Arc<LoadedNode>),
}

impl Deref for NodeRef<'_> {
    type Target = Node;

    fn deref(&self) -> &Node {
        match self {
            NodeRef::Borrowed(node) => node,
            NodeRef::Loaded(loaded) => &loaded.node,
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
