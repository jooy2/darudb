//! The names the object layer gives what it keeps in the storage kernel
//! (`design/objects.md`, "Storage"): a tree for each collection's objects
//! and one for each index, named by a prefix and the id, and the engine's
//! own tree of the stored schema and the auto-increment counters. They
//! begin with a NUL character, which the kernel's public calls refuse.
//!
//! [`CollectionNames`] holds a collection's names, made once for a
//! handle's schema, for the reads and writes of single objects, which name
//! a tree or two each.

use super::schema::CollectionDef;

/// The tree of the object layer's own records: the stored schema and the
/// auto-increment counters.
pub(crate) const META: &str = "\0meta";

/// The key of the stored schema in [`META`].
pub(crate) const SCHEMA_KEY: &[u8] = b"schema";

/// What the name of every tree of a collection's objects begins with.
pub(crate) const RECORDS_PREFIX: &str = "\0rec/";

/// What the name of every tree of an index begins with.
pub(crate) const INDEX_PREFIX: &str = "\0idx/";

/// The tree of collection `id`'s objects.
pub(crate) fn records(id: u64) -> IdName {
    IdName::new(RECORDS_PREFIX, id)
}

/// The tree of index `id`.
pub(crate) fn index_tree(id: u64) -> IdName {
    IdName::new(INDEX_PREFIX, id)
}

/// A name made of a prefix and an id, kept inline: the name of a tree of the
/// object layer, or the key of a counter in [`META`]. Every read and write of
/// an object names a tree or two, and an insert reads and stores its
/// collection's counter; a name formatted on the heap each time cost more
/// than the lookup it served.
#[derive(Clone, Copy)]
pub(crate) struct IdName {
    bytes: [u8; 32],
    len: usize,
}

impl IdName {
    fn new(prefix: &str, id: u64) -> Self {
        let mut digits = [0u8; 20];
        let mut start = digits.len();
        let mut rest = id;

        loop {
            start -= 1;
            // A remainder of a division by ten is a digit.
            digits[start] = b'0' + (rest % 10) as u8;
            rest /= 10;

            if rest == 0 {
                break;
            }
        }

        let digits = &digits[start..];
        let len = prefix.len() + digits.len();
        let mut bytes = [0u8; 32];

        bytes[..prefix.len()].copy_from_slice(prefix.as_bytes());
        bytes[prefix.len()..len].copy_from_slice(digits);

        Self { bytes, len }
    }
}

impl std::ops::Deref for IdName {
    type Target = str;

    fn deref(&self) -> &str {
        // A prefix and digits, both ASCII, so this never fails.
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }
}

impl std::fmt::Debug for IdName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, formatter)
    }
}

/// The key of collection `id`'s next auto-increment number in [`META`].
pub(crate) fn counter(id: u64) -> IdName {
    IdName::new("next/", id)
}

/// The names of a collection's trees and of its counter's key in [`META`],
/// made once for a handle's schema. Making a name for every object read or
/// written, and checking that it is text each time it was used, took about
/// a twenty-fifth of an insert and up to a thirtieth of a read by key.
#[derive(Debug)]
pub(crate) struct CollectionNames {
    /// The tree of the collection's objects.
    pub(crate) records: String,
    /// The key of the collection's counter.
    pub(crate) counter: String,
    /// The tree of each index, at the index's position in the collection.
    pub(crate) indexes: Vec<String>,
}

impl CollectionNames {
    pub(crate) fn of(collection: &CollectionDef) -> Self {
        Self {
            records: String::from(&*records(collection.id)),
            counter: String::from(&*counter(collection.id)),
            indexes: collection
                .indexes
                .iter()
                .map(|index| String::from(&*index_tree(index.id)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{counter, index_tree, records};

    #[test]
    fn a_tree_name_is_its_prefix_and_its_id_in_decimal() {
        for id in [0, 7, 10, 99, 4096, u64::MAX] {
            assert_eq!(&*records(id), format!("\0rec/{id}"));
            assert_eq!(&*index_tree(id), format!("\0idx/{id}"));
            assert_eq!(&*counter(id), format!("next/{id}"));
        }
    }
}
