//! Turning the pages a transaction changed into bytes, at commit time.
//!
//! A parent records each child's check, so the pages are encoded from the
//! leaves up. The pages are not written here: the commit writes all of them
//! together, in page order, before its barrier.

use super::node::{Branch, Child, LoadedNode, Node};
use super::read::internal;
use crate::error::Result;
use crate::format::{PageHeader, PageKind, Pointer, encode_branch};
use crate::storage::Pager;

/// One encoded page of a commit, ready to write.
#[derive(Debug)]
pub(crate) struct FinishedPage {
    pub(crate) page: u64,
    pub(crate) bytes: Vec<u8>,
    pub(crate) pointer: Pointer,
    /// The node as the page cache keeps it, from the page before sealing,
    /// which in an encrypted file is the last time it is plain.
    pub(crate) loaded: LoadedNode,
}

/// Encodes every page of `child` this transaction holds, children first, and
/// returns the pointer its parent records. `pager` seals each page, which in
/// an encrypted file means encrypting it: the check is the tag.
pub(crate) fn finish(
    pager: &Pager,
    txn: u64,
    tree: u64,
    child: Child,
    out: &mut Vec<FinishedPage>,
) -> Result<Pointer> {
    let (page, node) = match child {
        Child::Clean(pointer) => return Ok(pointer),
        Child::Dirty { page, node } => (page, *node),
    };
    let (header, mut bytes) = match node {
        // A leaf is laid out as its page already.
        Node::Leaf(leaf) => (
            header(PageKind::Leaf, 0, leaf.len(), txn, tree)?,
            leaf.into_page(),
        ),
        Node::Branch(Branch {
            level,
            keys,
            children,
        }) => {
            let mut pointers = Vec::with_capacity(children.len());

            for child in children {
                pointers.push(finish(pager, txn, tree, child, out)?);
            }

            let mut bytes = vec![0u8; pager.page_size()];

            encode_branch(&keys, &pointers, &mut bytes);

            (
                header(PageKind::Branch, level, keys.len(), txn, tree)?,
                bytes,
            )
        }
    };

    header.write(&mut bytes);

    let loaded = LoadedNode::encoded(bytes.clone(), &header);
    let check = pager.seal(page, &mut bytes)?;
    let pointer = Pointer { page, txn, check };

    out.push(FinishedPage {
        page,
        bytes,
        pointer,
        loaded,
    });

    Ok(pointer)
}

fn header(kind: PageKind, level: u8, count: usize, txn: u64, tree: u64) -> Result<PageHeader> {
    Ok(PageHeader {
        kind,
        level,
        count: u16::try_from(count).map_err(|_| internal("more entries than a page can count"))?,
        txn,
        tree,
        index: 0,
    })
}
