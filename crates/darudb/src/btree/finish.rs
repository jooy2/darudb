//! Turning the pages a transaction changed into bytes, at commit time.
//!
//! A parent records each child's check, so the pages are encoded from the
//! leaves up. The pages are not written here: the commit writes all of them
//! together, in page order, before its barrier.

use super::node::{Branch, Child, Node};
use super::read::internal;
use crate::error::Result;
use crate::format::{PageHeader, PageKind, Pointer, encode_branch, encode_leaf};
use crate::storage::Pager;

/// One encoded page of a commit, ready to write.
#[derive(Debug)]
pub(crate) struct FinishedPage {
    pub(crate) page: u64,
    pub(crate) tree: u64,
    pub(crate) bytes: Vec<u8>,
    pub(crate) pointer: Pointer,
    /// The node, with every child committed, for the page cache.
    pub(crate) node: Node,
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
    let mut bytes = vec![0u8; pager.page_size()];
    let (header, node) = match node {
        Node::Leaf(entries) => {
            encode_leaf(&entries, &mut bytes);

            (
                header(PageKind::Leaf, 0, entries.len(), txn, tree)?,
                Node::Leaf(entries),
            )
        }
        Node::Branch(Branch {
            level,
            keys,
            children,
        }) => {
            let mut pointers = Vec::with_capacity(children.len());

            for child in children {
                pointers.push(finish(pager, txn, tree, child, out)?);
            }

            encode_branch(&keys, &pointers, &mut bytes);

            (
                header(PageKind::Branch, level, keys.len(), txn, tree)?,
                Node::Branch(Branch {
                    level,
                    keys,
                    children: pointers.into_iter().map(Child::Clean).collect(),
                }),
            )
        }
    };

    header.write(&mut bytes);

    let check = pager.seal(page, &mut bytes)?;
    let pointer = Pointer { page, txn, check };

    out.push(FinishedPage {
        page,
        tree,
        bytes,
        pointer,
        node,
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
