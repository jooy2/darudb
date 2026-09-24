//! The object layer: collections of typed objects kept in the storage
//! kernel's trees, declared by a schema and migrated from one schema version
//! to the next (`design/objects.md`).
//!
//! - `declare`: what an application declares, [`Schema`] and [`Migration`].
//! - `resolve`: comparing a declared schema with the stored one, and working
//!   out what a migration changes. Computation only, no transaction.
//! - `open`: storing, checking and migrating the schema when a file opens,
//!   and [`Migrating`], which migration functions get.
//! - `objects`: reading and writing objects, and keeping every index in step
//!   with the objects in the same transaction.
//!
//! The encodings of keys, records and the stored schema are in
//! `format::object`, below the kernel, since they are functions over bytes.
//! The object layer only reaches the kernel through transactions, in trees
//! whose names begin with a NUL character, which applications cannot touch.

mod declare;
pub(crate) mod objects;
mod open;
mod resolve;
#[cfg(test)]
mod tests;

pub use declare::{Collection, Embedded, Migration, Schema, Type};
pub use objects::{CollectionReader, CollectionWriter};
pub use open::Migrating;
pub(crate) use open::{Opened, Pending, check, open};

use crate::error::Result;
use crate::txn::{ReadTransaction, WriteTransaction};

impl ReadTransaction {
    /// Collection `name` of the schema the database was opened with
    /// ([`OpenOptions::schema`](crate::OpenOptions::schema)).
    ///
    /// It fails with [`Error::InvalidArgument`](crate::Error::InvalidArgument)
    /// if the schema has no such collection, or the database was opened
    /// without one, and with
    /// [`Error::SchemaMismatch`](crate::Error::SchemaMismatch) if another
    /// process has migrated the file to another schema since.
    pub fn collection(&self, name: &str) -> Result<CollectionReader<'_>> {
        CollectionReader::new(self, name)
    }
}

impl WriteTransaction {
    /// Collection `name` of the schema the database was opened with, for
    /// reading and writing its objects. It fails as
    /// [`ReadTransaction::collection`] does.
    pub fn collection(&mut self, name: &str) -> Result<CollectionWriter<'_>> {
        CollectionWriter::new(self, name)
    }
}
