//! Queries over the objects of a collection (`design/objects.md`, "Queries"
//! and "Running a query").
//!
//! - `ir`: the tree every query becomes, and its encoding for the language
//!   boundary.
//! - `build`: [`Query`] and [`Filter`], which build that tree in Rust.
//! - `parse`: the query language, which builds the same tree from text.
//! - `plan`: checking a query against the schema, and choosing whether a key
//!   range, an index or a walk of every object finds its objects.
//! - `run`: reading the objects, testing them, sorting, and counting.
//!
//! Whatever the plan, a query gives the objects a walk of every object would,
//! in the same order; `tests` checks that on random data and random queries.

mod build;
pub(crate) mod ir;
mod parse;
mod plan;
mod run;
#[cfg(test)]
mod tests;

pub use build::{Filter, Query};

use crate::error::Result;
use crate::format::object::Object;
use crate::schema::{CollectionReader, CollectionWriter};

impl CollectionReader<'_> {
    /// The objects `query` finds, in its order.
    ///
    /// It fails with [`Error::InvalidQuery`](crate::Error::InvalidQuery) if
    /// the query names a field the collection does not have, or tests one
    /// with a value of another type.
    pub fn query(&self, query: &Query) -> Result<Vec<Object>> {
        let (source, schema, collection) = self.parts();

        run::objects(source, &plan::plan(schema, collection, &query.ir)?)
    }

    /// How many objects `query` finds, after its offset and within its
    /// limit. A filter that an index or the primary key answers alone is
    /// counted without reading the objects.
    pub fn count(&self, query: &Query) -> Result<u64> {
        let (source, schema, collection) = self.parts();

        run::count(source, &plan::plan(schema, collection, &query.ir)?)
    }
}

impl CollectionWriter<'_> {
    /// The objects `query` finds, in its order, with this transaction's
    /// changes. It fails as [`CollectionReader::query`] does.
    pub fn query(&self, query: &Query) -> Result<Vec<Object>> {
        let (source, schema, collection) = self.parts();

        run::objects(source, &plan::plan(schema, collection, &query.ir)?)
    }

    /// How many objects `query` finds, with this transaction's changes; see
    /// [`CollectionReader::count`].
    pub fn count(&self, query: &Query) -> Result<u64> {
        let (source, schema, collection) = self.parts();

        run::count(source, &plan::plan(schema, collection, &query.ir)?)
    }
}
