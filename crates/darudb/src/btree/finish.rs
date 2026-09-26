//! Turning the pages a transaction changed into bytes, at commit time.
//!
//! A parent records each child's check, so the pages are encoded from the
//! leaves up. The pages are not written here: the commit writes all of them
//! together, in page order, before its barrier.

use super::leaf::LeafParts;
use super::node::{Branch, Child, LoadedNode, Node};
use super::read::internal;
use crate::error::Result;
use crate::format::{PageHeader, PageKind, Pointer, encode_branch};
use crate::storage::Pager;

/// One encoded page of a commit, ready to write.
#[derive(Debug)]
pub(crate) struct FinishedPage {
    pub(crate) page: u64,
    /// The sealed page, in an encrypted file. A plain file's sealed page is
    /// the page the cached node keeps, which is written instead: copying
    /// every page of a commit for the cache took a twentieth of a small
    /// deferred commit.
    sealed: Option<Vec<u8>>,
    pub(crate) pointer: Pointer,
    /// The node as the page cache keeps it, from the page before sealing,
    /// which in an encrypted file is the last time it is plain.
    pub(crate) loaded: LoadedNode,
}

impl FinishedPage {
    /// The bytes to write.
    pub(crate) fn bytes(&self) -> &[u8] {
        self.sealed.as_deref().unwrap_or_else(|| self.loaded.page())
    }
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
    let (header, mut bytes, heads, size, low) = match node {
        // A leaf is laid out as its page already.
        Node::Leaf(leaf) => {
            let header = header(PageKind::Leaf, 0, leaf.len(), txn, tree)?;
            let LeafParts {
                page,
                heads,
                size,
                low,
            } = leaf.into_parts();

            (header, page, heads, size, low)
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

            let mut bytes = vec![0u8; pager.page_size()];

            encode_branch(keys.iter(), &pointers, &mut bytes);

            let header = header(PageKind::Branch, level, keys.len(), txn, tree)?;
            let size = keys.branch_len();

            (header, bytes, keys.into_heads(), size, 0)
        }
    };

    header.write(&mut bytes);

    // A plain page is sealed by writing its check after its content, which
    // the cached node never reads.
    let (check, sealed, loaded) = if pager.is_encrypted() {
        let loaded = LoadedNode::encoded(bytes.clone(), &header, heads, size, low);
        let check = pager.seal(page, &mut bytes)?;

        (check, Some(bytes), loaded)
    } else {
        let check = pager.seal(page, &mut bytes)?;

        (
            check,
            None,
            LoadedNode::encoded(bytes, &header, heads, size, low),
        )
    };
    let pointer = Pointer { page, txn, check };

    out.push(FinishedPage {
        page,
        sealed,
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
