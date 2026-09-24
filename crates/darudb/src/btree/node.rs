//! A B+tree node in memory, and the reference a branch or a root keeps to it.

use std::cmp::Ordering;
use std::sync::Arc;

use super::leaf::Leaf;
use crate::format::{
    PageHeader, PageKind, Pointer, StoredRef, branch_child, branch_key, branch_len, branch_size,
    check_branch, check_leaf, decode_branch, leaf_entry, leaf_inline, leaf_key, leaf_size,
    leaf_value,
};
use crate::storage::Weigh;

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
///
/// Beside the page, the node keeps the **head** of every key: the four bytes
/// after the prefix all its keys share, padded with zeros, as one number. A
/// search compares heads, which lie together in one small array, and reads a
/// key only where its head equals the head of the key it looks for. Each
/// step of a search on the page alone follows an offset to a key anywhere in
/// the page, and that memory access, rather than the comparison, is what a
/// lookup spent its time on. The heads cost four bytes a key in memory and
/// nothing on disk, where they would take room from the entries.
#[derive(Debug)]
pub(crate) struct LoadedNode {
    pub(crate) tree: u64,
    pub(crate) txn: u64,
    level: u8,
    leaf: bool,
    /// The bytes of the page's content the node takes, as [`Node::len`]
    /// counts them.
    size: u32,
    /// The length of the prefix every key begins with, which the heads
    /// follow: the prefix the first and last keys share.
    prefix: u16,
    /// The prefix, when it is at most [`KEPT_PREFIX`] bytes long. A search
    /// compares the key it looks for with it here, next to the node's other
    /// fields, rather than with the first key at the far end of the page.
    kept_prefix: [u8; KEPT_PREFIX],
    /// The head of each key: as many as a leaf has entries, or a branch
    /// keys.
    heads: Box<[u32]>,
    page: Box<[u8]>,
}

/// The longest prefix a [`LoadedNode`] keeps a copy of. Eight bytes cover
/// the prefix of a tree of integer keys and keep the node in as few cache
/// lines as it took without heads.
const KEPT_PREFIX: usize = 8;

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
        let key_at = |index| {
            if leaf {
                leaf_key(&page, index)
            } else {
                branch_key(&page, count, index)
            }
        };
        let (prefix, mut kept_prefix) = (
            match count {
                0 => 0,
                _ => shared_prefix(key_at(0), key_at(count - 1)),
            },
            [0; KEPT_PREFIX],
        );

        if count > 0 && prefix <= KEPT_PREFIX {
            kept_prefix[..prefix].copy_from_slice(&key_at(0)[..prefix]);
        }

        let heads = (0..count)
            .map(|index| key_head(key_at(index), prefix))
            .collect();
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
            // A page and a key are at most 65536 bytes long.
            size: u32::try_from(size).unwrap_or(u32::MAX),
            prefix: u16::try_from(prefix).unwrap_or(u16::MAX),
            kept_prefix,
            heads,
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
            Ok(Node::Leaf(Leaf::from_page(&self.page, self.count())))
        } else {
            decode_branch(&self.page, self.count()).map(|(keys, children)| {
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

    /// A leaf's entries, or a branch's keys.
    fn count(&self) -> usize {
        self.heads.len()
    }

    /// The prefix every key begins with.
    fn prefix(&self) -> &[u8] {
        match usize::from(self.prefix) {
            prefix if prefix <= KEPT_PREFIX => &self.kept_prefix[..prefix],
            prefix if self.leaf => &leaf_key(&self.page, 0)[..prefix],
            prefix => &branch_key(&self.page, self.count(), 0)[..prefix],
        }
    }
}

impl Weigh for LoadedNode {
    /// The page and the heads, which the page cache counts against its size.
    fn weight(&self) -> usize {
        self.page.len() + size_of::<u32>() * self.heads.len()
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
            NodeRef::Loaded(loaded) => loaded.count(),
        }
    }

    /// The bytes of a page's content the node takes.
    pub(crate) fn size(&self) -> usize {
        match self {
            NodeRef::Borrowed(node) => node.len(),
            NodeRef::Loaded(loaded) => loaded.size as usize,
        }
    }

    /// Key `index`: a leaf entry's, or a branch's separator.
    pub(crate) fn key(&self, index: usize) -> &[u8] {
        match self {
            NodeRef::Borrowed(Node::Leaf(leaf)) => leaf.key(index),
            NodeRef::Borrowed(Node::Branch(branch)) => &branch.keys[index],
            NodeRef::Loaded(loaded) if loaded.leaf => leaf_key(&loaded.page, index),
            NodeRef::Loaded(loaded) => branch_key(&loaded.page, loaded.count(), index),
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
        // Every lookup runs this on each level, so the kind of node is
        // settled once rather than on every probe.
        match self {
            NodeRef::Loaded(loaded) if loaded.leaf => {
                let page = &loaded.page;

                search_heads(&loaded.heads, loaded.prefix(), key, or_equal, |at| {
                    leaf_key(page, at)
                })
            }
            NodeRef::Loaded(loaded) => {
                let (page, count) = (&loaded.page, loaded.count());

                search_heads(&loaded.heads, loaded.prefix(), key, or_equal, |at| {
                    branch_key(page, count, at)
                })
            }
            NodeRef::Borrowed(_) => search(self.count(), key, or_equal, |at| self.key(at)),
        }
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

/// The head of `key` in a node whose keys share their first `prefix` bytes:
/// the four bytes after them, padded with zeros, as a big-endian number.
///
/// Heads order keys that share the prefix, though not strictly: a key orders
/// before another when its head is lower and after it when its head is
/// higher, and only a comparison of the keys tells when the heads are equal.
fn key_head(key: &[u8], prefix: usize) -> u32 {
    let rest = key.get(prefix..).unwrap_or_default();
    let mut head = [0u8; 4];
    let len = rest.len().min(4);

    head[..len].copy_from_slice(&rest[..len]);

    u32::from_be_bytes(head)
}

/// The length of the prefix `a` and `b` share.
fn shared_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(a, b)| a == b).count()
}

/// How many of the ascending keys of a node, whose heads are `heads`, are
/// below `key`, or at or below it with `or_equal`: a binary search over the
/// heads, which reads a key through `key_at` only where its head equals
/// `key`'s. Every key begins with `shared`.
#[inline(always)]
fn search_heads<'k>(
    heads: &[u32],
    shared: &[u8],
    key: &[u8],
    or_equal: bool,
    key_at: impl Fn(usize) -> &'k [u8],
) -> usize {
    // A key that does not begin with the prefix is below every key of the
    // node or above them all.
    if key.get(..shared.len()) != Some(shared) {
        return if key < shared { 0 } else { heads.len() };
    }

    let prefix = shared.len();
    let (head, rest) = (key_head(key, prefix), &key[prefix..]);
    let (mut low, mut high) = (0, heads.len());

    while low < high {
        let middle = low + (high - low) / 2;
        let below = match heads[middle].cmp(&head) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => match compare(&key_at(middle)[prefix..], rest) {
                Ordering::Less => true,
                Ordering::Equal => or_equal,
                Ordering::Greater => false,
            },
        };

        if below {
            low = middle + 1;
        } else {
            high = middle;
        }
    }

    low
}

/// How many of `count` ascending keys, which `key_at` gives by position,
/// are below `key`, or at or below it with `or_equal`: a binary search.
#[inline(always)]
fn search<'k>(
    count: usize,
    key: &[u8],
    or_equal: bool,
    key_at: impl Fn(usize) -> &'k [u8],
) -> usize {
    let (mut low, mut high) = (0, count);

    while low < high {
        let middle = low + (high - low) / 2;
        let below = match compare(key_at(middle), key) {
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

/// Compares two keys as unsigned bytes, the first eight as one number, which
/// settles most comparisons of a search without a call to compare memory.
#[inline(always)]
fn compare(a: &[u8], b: &[u8]) -> Ordering {
    match (a.split_first_chunk::<8>(), b.split_first_chunk::<8>()) {
        (Some((a_head, a_rest)), Some((b_head, b_rest))) => u64::from_be_bytes(*a_head)
            .cmp(&u64::from_be_bytes(*b_head))
            .then_with(|| a_rest.cmp(b_rest)),
        _ => a.cmp(b),
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
    use crate::format::{
        Check, LeafEntry, StoredValue, branch_len, content_len, encode_branch, encode_leaf,
    };
    use crate::testing::Rng;

    /// A key of `shared`, then up to six bytes from a short alphabet with
    /// zeros in it, so that keys often end inside their heads, or share them
    /// when their tails differ only in zeros.
    fn key_after(rng: &mut Rng, shared: &[u8]) -> Vec<u8> {
        let mut key = shared.to_vec();

        key.extend((0..rng.index(7)).map(|_| [0x00, 0x01, 0x7F, 0xFF][rng.index(4)]));

        key
    }

    /// Keys to search for: the node's own, keys between them, keys without
    /// the node's prefix, the prefix itself and keys shorter than it.
    fn probes(rng: &mut Rng, keys: &[Vec<u8>], shared: &[u8]) -> Vec<Vec<u8>> {
        let mut probes: Vec<Vec<u8>> = keys.to_vec();

        probes.extend((0..keys.len()).map(|_| key_after(rng, shared)));
        probes.extend((0..=shared.len()).map(|len| shared[..len].to_vec()));

        for at in 0..shared.len() {
            for byte in [0x00, 0xFF] {
                let mut other = shared.to_vec();

                other[at] = byte;
                probes.push(other.clone());
                other.push(0x01);
                probes.push(other);
            }
        }

        probes.push(Vec::new());
        probes.push(vec![0xFF; shared.len() + 8]);
        probes
    }

    /// A random node's worth of distinct ascending keys that begin with
    /// `shared`, as many as fit in a page of `page_size` bytes.
    fn node_keys(rng: &mut Rng, shared: &[u8], page_size: usize) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = (0..1 + rng.index(120))
            .map(|_| key_after(rng, shared))
            .collect();

        keys.sort();
        keys.dedup();

        while branch_len(&keys) > content_len(page_size) {
            keys.pop();
        }

        keys
    }

    fn loaded(page: Vec<u8>, kind: PageKind, count: usize) -> NodeRef<'static> {
        let header = PageHeader {
            kind,
            level: u8::from(kind == PageKind::Branch),
            count: u16::try_from(count).unwrap(),
            txn: 1,
            tree: 16,
            index: 0,
        };
        let mut page = page;

        header.write(&mut page);

        NodeRef::Loaded(Arc::new(LoadedNode::read(page, &header).unwrap()))
    }

    /// Every search of a leaf or a branch through the heads finds the
    /// position a search through the keys alone finds, with prefixes shorter
    /// and longer than the copy a node keeps.
    #[test]
    fn a_search_through_the_heads_finds_what_the_keys_say() {
        const PAGE: usize = 4096;

        let mut rng = Rng::new(17);

        for round in 0..600 {
            let shared: Vec<u8> = match round % 5 {
                0 => Vec::new(),
                1 => b"abc".to_vec(),
                2 => b"\x04\x80\0\0\0\0\0\x01".to_vec(),
                3 => b"\x04\x80\0\0\0\0\0\x01\0".to_vec(),
                _ => rng.bytes(20),
            };
            let keys = node_keys(&mut rng, &shared, PAGE);
            let mut leaf = vec![0u8; PAGE];
            let entries: Vec<LeafEntry> = keys
                .iter()
                .map(|key| LeafEntry {
                    key: key.clone(),
                    value: StoredValue::Inline(Vec::new()),
                })
                .collect();

            assert!(entries.iter().map(LeafEntry::len).sum::<usize>() <= content_len(PAGE));
            encode_leaf(&entries, &mut leaf);

            let mut branch = vec![0u8; PAGE];
            let children: Vec<Pointer> = (0..=keys.len() as u64)
                .map(|page| Pointer {
                    page: page + 1,
                    txn: 1,
                    check: Check::ZERO,
                })
                .collect();

            encode_branch(&keys, &children, &mut branch);

            let nodes = [
                loaded(leaf, PageKind::Leaf, keys.len()),
                loaded(branch, PageKind::Branch, keys.len()),
            ];

            for probe in probes(&mut rng, &keys, &shared) {
                for node in &nodes {
                    assert_eq!(
                        node.rank(&probe, false),
                        keys.partition_point(|key| key < &probe),
                        "{probe:?} in {keys:?}"
                    );
                    assert_eq!(
                        node.rank(&probe, true),
                        keys.partition_point(|key| key <= &probe),
                        "{probe:?} in {keys:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn keys_compare_as_unsigned_bytes() {
        let mut rng = Rng::new(3);
        // Short alphabets and lengths around eight, so that keys often share
        // their first eight bytes or end inside them.
        let key = |rng: &mut Rng| -> Vec<u8> {
            let len = rng.index(20);

            (0..len)
                .map(|_| [0x00, 0x01, 0x7F, 0x80, 0xFF][rng.index(5)])
                .collect()
        };

        for _ in 0..20_000 {
            let (a, b) = (key(&mut rng), key(&mut rng));

            assert_eq!(compare(&a, &b), a.cmp(&b), "{a:?} and {b:?}");
            assert_eq!(compare(&a, &a), Ordering::Equal);
        }
    }

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
