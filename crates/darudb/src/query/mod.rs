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

pub use build::{Filter, Query, QueryRequest};

use crate::error::Result;
use crate::format::object::Object;
use crate::format::object::codec::NameOrder;
use crate::format::object::schema::{CollectionDef, StoredSchema};
use crate::schema::objects::{Decoder, Source};
use crate::schema::{CollectionReader, CollectionWriter};

/// `query` on `collection`, checked and planned, with its parameters'
/// values if it is a bound prepared query.
fn plan<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    query: &'s Query,
) -> Result<plan::Plan<'s>> {
    plan::plan(schema, collection, query.ir(), parameters(query))
}

/// The values of `query`'s parameters, none if it is not a bound prepared
/// query.
fn parameters(query: &Query) -> &[crate::format::object::Value] {
    query.parameters.as_deref().unwrap_or_default()
}

/// Gives `take` the record of each object `query` finds, as
/// [`run::each_stored`] does: through [`run::point`] when it is a lookup of
/// one value, and planned otherwise.
fn each_stored(
    source: &dyn Source,
    schema: &StoredSchema,
    collection: &CollectionDef,
    query: &Query,
    take: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    if run::point(source, collection, query.ir(), parameters(query), take)? {
        return Ok(());
    }

    run::each_stored(source, &plan(schema, collection, query)?, take)
}

/// The objects `query` finds, as [`run::objects`] gives them, through
/// [`run::point`] when it is a lookup of one value.
fn objects(
    source: &dyn Source,
    schema: &StoredSchema,
    collection: &CollectionDef,
    query: &Query,
    order: Option<&NameOrder>,
) -> Result<Vec<Object>> {
    let mut found = Vec::new();
    let mut decoder = Decoder::new(collection, order);
    let point = run::point(
        source,
        collection,
        query.ir(),
        parameters(query),
        &mut |record| {
            found.push(decoder.decode(source, record)?);

            Ok(())
        },
    )?;

    if point {
        return Ok(found);
    }

    run::objects(source, &plan(schema, collection, query)?, order)
}

/// The records `query` finds, as [`run::stored`] gives them, through
/// [`run::point`] when it is a lookup of one value.
fn stored(
    source: &dyn Source,
    schema: &StoredSchema,
    collection: &CollectionDef,
    query: &Query,
) -> Result<Vec<Vec<u8>>> {
    let mut found = Vec::new();
    let point = run::point(
        source,
        collection,
        query.ir(),
        parameters(query),
        &mut |record| {
            found.push(record.to_vec());

            Ok(())
        },
    )?;

    if point {
        return Ok(found);
    }

    run::stored(source, &plan(schema, collection, query)?)
}

impl CollectionReader<'_> {
    /// The objects `query` finds, in its order.
    ///
    /// It fails with [`Error::InvalidQuery`](crate::Error::InvalidQuery) if
    /// the query names a field the collection does not have, or tests one
    /// with a value of another type.
    pub fn query(&self, query: &Query) -> Result<Vec<Object>> {
        let (source, schema, collection) = self.parts();

        objects(source, schema, collection, query, self.order())
    }

    /// How many objects `query` finds, after its offset and within its
    /// limit. A filter that an index or the primary key answers alone is
    /// counted without reading the objects.
    pub fn count(&self, query: &Query) -> Result<u64> {
        let (source, schema, collection) = self.parts();

        run::count(source, &plan(schema, collection, query)?)
    }

    /// The records of the objects `query` finds, in its order, as the file
    /// holds them (`design/objects.md`, "Records"), for a language binding
    /// that decodes them itself. An object the filter and the sort need not
    /// read is not decoded at all.
    pub fn query_records(&self, query: &Query) -> Result<Vec<Vec<u8>>> {
        let (source, schema, collection) = self.parts();

        stored(source, schema, collection, query)
    }

    /// Gives `visit` the record of each object `query` finds, in its order,
    /// as [`query_records`](Self::query_records) returns them, but borrowed
    /// rather than copied into a vector of its own: a binding that copies
    /// the records into one buffer copies each once. It stops at the first
    /// error `visit` returns.
    pub fn query_records_with(
        &self,
        query: &Query,
        mut visit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let (source, schema, collection) = self.parts();

        each_stored(source, schema, collection, query, &mut visit)
    }
}

impl CollectionWriter<'_> {
    /// The objects `query` finds, in its order, with this transaction's
    /// changes. It fails as [`CollectionReader::query`] does.
    pub fn query(&self, query: &Query) -> Result<Vec<Object>> {
        let (source, schema, collection) = self.parts();

        objects(source, schema, collection, query, self.order())
    }

    /// How many objects `query` finds, with this transaction's changes; see
    /// [`CollectionReader::count`].
    pub fn count(&self, query: &Query) -> Result<u64> {
        let (source, schema, collection) = self.parts();

        run::count(source, &plan(schema, collection, query)?)
    }

    /// The records of the objects `query` finds, with this transaction's
    /// changes; see [`CollectionReader::query_records`].
    pub fn query_records(&self, query: &Query) -> Result<Vec<Vec<u8>>> {
        let (source, schema, collection) = self.parts();

        stored(source, schema, collection, query)
    }

    /// Gives `visit` the record of each object `query` finds, with this
    /// transaction's changes; see [`CollectionReader::query_records_with`].
    pub fn query_records_with(
        &self,
        query: &Query,
        mut visit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let (source, schema, collection) = self.parts();

        each_stored(source, schema, collection, query, &mut visit)
    }
}
