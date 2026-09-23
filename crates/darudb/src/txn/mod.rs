//! Read and write transactions, commits, and recovery.
//!
//! A read transaction sees one commit, its snapshot, for as long as it lives.
//! A write transaction starts from the published commit and ends with a new
//! commit or with nothing; there is one at a time per file.
//! `design/commits-and-recovery.md` is the specification.
//!
//! The storage kernel stores named trees of byte keys and byte values. The
//! object and query layers of phase 4 build on these.

mod commit;
mod read;
pub(crate) mod recovery;
mod write;

use std::ops::{Bound, RangeBounds};

use crate::btree::{self, Child, Loader};
use crate::error::{Error, Result};
use crate::format::{CATALOG_TREE, Pointer, TreeDescriptor, max_key_len};

pub use read::ReadTransaction;
pub use write::WriteTransaction;

/// Entries of a tree in key order, as [`ReadTransaction::range`] and
/// [`WriteTransaction::range`] return them.
///
/// Each item is a key and its value. An item is an error when a page the walk
/// needs is damaged; the walk ends there.
#[derive(Debug)]
pub struct Range<'a> {
    inner: Option<btree::Range<'a, Loader>>,
}

impl Iterator for Range<'_> {
    type Item = Result<(Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.as_mut()?.next()
    }
}

impl<'a> Range<'a> {
    fn empty() -> Self {
        Self { inner: None }
    }

    fn over<K: AsRef<[u8]>>(
        loader: &'a Loader,
        tree: u64,
        root: Option<&'a Child>,
        bounds: &impl RangeBounds<K>,
    ) -> Result<Self> {
        Ok(Self {
            inner: Some(btree::Range::new(
                loader,
                tree,
                root,
                as_bytes(bounds.start_bound()),
                as_bytes(bounds.end_bound()),
            )?),
        })
    }

    fn over_committed<K: AsRef<[u8]>>(
        loader: &'a Loader,
        tree: u64,
        root: Pointer,
        bounds: &impl RangeBounds<K>,
    ) -> Result<Self> {
        Ok(Self {
            inner: Some(btree::Range::from_pointer(
                loader,
                tree,
                root,
                as_bytes(bounds.start_bound()),
                as_bytes(bounds.end_bound()),
            )?),
        })
    }
}

/// A bound on any byte-like key as a bound on bytes.
fn as_bytes<K: AsRef<[u8]>>(bound: Bound<&K>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(key) => Bound::Included(key.as_ref()),
        Bound::Excluded(key) => Bound::Excluded(key.as_ref()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

/// A tree name as the catalog stores it, once it is known to be one.
fn tree_key(name: &str, page_size: usize) -> Result<&[u8]> {
    if name.is_empty() || name.starts_with('\0') || name.len() > max_key_len(page_size) {
        return Err(Error::InvalidArgument {
            message: format!(
                "a tree name is 1 to {} bytes long and does not start with a NUL character",
                max_key_len(page_size)
            ),
        });
    }

    Ok(name.as_bytes())
}

/// A key the tree can hold.
fn check_key(key: &[u8], page_size: usize) -> Result<()> {
    if key.len() > max_key_len(page_size) {
        return Err(Error::InvalidArgument {
            message: format!(
                "a key is at most {} bytes long with this page size, not {}",
                max_key_len(page_size),
                key.len()
            ),
        });
    }

    Ok(())
}

/// A value the tree can hold.
fn check_value(value: &[u8]) -> Result<()> {
    if u32::try_from(value.len()).is_err() {
        return Err(Error::InvalidArgument {
            message: format!("a value is less than 4 GiB long, not {} bytes", value.len()),
        });
    }

    Ok(())
}

/// The descriptor the catalog rooted at `catalog` keeps for `name`.
fn find_tree(
    loader: &Loader,
    catalog: Option<&Child>,
    name: &[u8],
) -> Result<Option<TreeDescriptor>> {
    match btree::get(loader, CATALOG_TREE, catalog, name)? {
        None => Ok(None),
        Some(value) => TreeDescriptor::decode(&value)
            .map(Some)
            .map_err(|reason| corrupted_catalog(loader, reason)),
    }
}

/// The names of every tree in the catalog rooted at `catalog`.
fn catalog_names(loader: &Loader, catalog: Option<&Child>) -> Result<Vec<String>> {
    btree::Range::new(
        loader,
        CATALOG_TREE,
        catalog,
        Bound::Unbounded,
        Bound::Unbounded,
    )?
    .map(|entry| {
        let (name, _) = entry?;

        String::from_utf8(name).map_err(|_| corrupted_catalog(loader, "a tree name is not UTF-8"))
    })
    .collect()
}

fn corrupted_catalog(loader: &Loader, reason: &str) -> Error {
    loader.corrupted_file(format!("the catalog: {reason}"))
}

/// The root pointer of a tree as a child to start a walk from.
fn root_child(root: Pointer) -> Option<Child> {
    (!root.is_null()).then_some(Child::Clean(root))
}
