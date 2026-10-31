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
//! - `keys`: sets of primary keys, which a run gathers from the indexes for
//!   the parts of a filter they answer alone.
//! - `aggregate`: sums, means, and the least and greatest values, worked out
//!   exactly from the objects a query finds.
//! - `change`: updating every object a query finds, in one call.
//! - `cascade`: deleting an object or every object a query finds, with the
//!   objects that cascading links take with them.
//! - `date`: dates as the query language writes them.
//! - `fold`: comparing strings ignoring case, by Unicode's simple case
//!   folding, whose table `fold_table` is generated from Unicode's data.
//! - `typed`: conditions, sorts and aggregates that name their fields
//!   through a type's field constants.
//!
//! Whatever the plan, a query gives the objects a walk of every object would,
//! in the same order; `tests` checks that on random data and random queries.

mod aggregate;
mod build;
mod cascade;
mod change;
mod date;
mod fold;
mod fold_table;
pub(crate) mod ir;
mod keys;
mod parse;
mod plan;
mod run;
#[cfg(test)]
mod tests;
mod typed;

pub use aggregate::{Aggregate, tagged};
pub use build::{Filter, IntoPath, Query, QueryRequest};
pub use change::ChangeWhere;
pub use typed::{ComparableType, LinkType, NestedType, NumberType, OrderedType, PathOf, TextType};

/// Traits the crate's public traits require, which only the crate
/// implements, so that a type outside it cannot take part in them.
mod sealed {
    /// A field type the typed conditions know.
    pub trait Typed {}

    /// A field a query sorts by.
    pub trait Path {}
}

use crate::error::Result;
use crate::format::object::codec::{self, NameOrder};
use crate::format::object::schema::{CollectionDef, StoredSchema};
use crate::format::object::{Object, Value};
use crate::schema::objects::{Decoder, Source};
use crate::schema::{CollectionReader, CollectionType, CollectionWriter, TypedReader, TypedWriter};

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

/// The primary keys of the objects `query` finds, in its order, gathered
/// before any of them changes: a walk borrows the transaction, which a
/// change of an object needs to itself.
pub(crate) fn found_keys(
    source: &dyn Source,
    schema: &StoredSchema,
    collection: &CollectionDef,
    query: &Query,
) -> Result<Vec<Value>> {
    let kind = &collection
        .key_field()
        .ok_or_else(|| crate::Error::Internal {
            message: format!("`{}` has no key field", collection.name),
        })?
        .kind;
    let mut keys = Vec::new();

    each_stored(source, schema, collection, query, &mut |record| {
        let found = codec::find_field(record, collection.key)
            .and_then(|found| found.ok_or("a record lacks its primary key"))
            .and_then(|found| codec::field_value(found, kind))
            .map_err(|reason| {
                source.corrupted(format!("an object of `{}`: {reason}", collection.name))
            })?;

        keys.push(found);

        Ok(())
    })?;

    Ok(keys)
}

/// `function` over the field at `path` of the objects `query` finds.
fn aggregate_at(
    source: &dyn Source,
    schema: &StoredSchema,
    collection: &CollectionDef,
    query: &Query,
    function: Aggregate,
    path: &[String],
) -> Result<Value> {
    let resolved = plan::aggregated(schema, collection, path, function)?;

    run::aggregate(
        source,
        &plan(schema, collection, query)?,
        &resolved,
        path,
        function,
    )
}

/// A path to a field as the builders take it: names joined by `.`.
fn dotted(field: &str) -> Vec<String> {
    field.split('.').map(str::to_owned).collect()
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

    /// Works out `function` over the values of the field at `path`, one name
    /// for each step, of the objects `query` finds, after its offset and
    /// within its limit, as a binding's IR asks for it. [`sum`](Self::sum),
    /// [`min`](Self::min), [`max`](Self::max) and [`avg`](Self::avg) take the
    /// path as text.
    ///
    /// Null values, and paths that reach none, are left out. The path reads
    /// one value of each object, through embedded objects and links but not
    /// lists; another fails with
    /// [`Error::InvalidQuery`](crate::Error::InvalidQuery), as a field of a
    /// type the function does not read does.
    pub fn aggregate(&self, query: &Query, function: Aggregate, path: &[String]) -> Result<Value> {
        let (source, schema, collection) = self.parts();

        aggregate_at(source, schema, collection, query, function, path)
    }

    /// The sum of `field`, an int or float field, or a path such as
    /// `order.total`, over the objects `query` finds: an int for an int
    /// field, exact, and a float for a float field, the exact sum rounded
    /// once. A sum of no values is zero. A sum of ints outside the range of
    /// an `i64` fails with
    /// [`Error::InvalidArgument`](crate::Error::InvalidArgument).
    pub fn sum(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Sum, &dotted(field))
    }

    /// The least value of `field`, an int, float, string or bytes field, over
    /// the objects `query` finds, in the order a sort gives; null when there
    /// is none. With nothing of the filter left to test, a field with an
    /// index is read from the index's first entry.
    pub fn min(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Min, &dotted(field))
    }

    /// The greatest value of `field`, as [`min`](Self::min) gives the least.
    pub fn max(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Max, &dotted(field))
    }

    /// The mean of `field`, an int or float field, over the objects `query`
    /// finds, as a float: the exact sum divided by the number of values,
    /// rounded once. Null when there are no values.
    pub fn avg(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Avg, &dotted(field))
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

    /// [`CollectionReader::aggregate`], with this transaction's changes.
    pub fn aggregate(&self, query: &Query, function: Aggregate, path: &[String]) -> Result<Value> {
        let (source, schema, collection) = self.parts();

        aggregate_at(source, schema, collection, query, function, path)
    }

    /// [`CollectionReader::sum`], with this transaction's changes.
    pub fn sum(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Sum, &dotted(field))
    }

    /// [`CollectionReader::min`], with this transaction's changes.
    pub fn min(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Min, &dotted(field))
    }

    /// [`CollectionReader::max`], with this transaction's changes.
    pub fn max(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Max, &dotted(field))
    }

    /// [`CollectionReader::avg`], with this transaction's changes.
    pub fn avg(&self, query: &Query, field: &str) -> Result<Value> {
        self.aggregate(query, Aggregate::Avg, &dotted(field))
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

impl<T: CollectionType> TypedReader<'_, T> {
    /// The objects `query` finds, in its order, read straight into `T`.
    ///
    /// It fails as [`CollectionReader::query`] does, and with
    /// [`Error::Corrupted`](crate::Error::Corrupted) for a record that does
    /// not read as `T`.
    pub fn query(&self, query: &Query) -> Result<Vec<T>> {
        let (source, schema, collection) = self.untyped().parts();
        let mut found = Vec::new();

        each_stored(source, schema, collection, query, &mut |record| {
            found.push(self.read(record)?);

            Ok(())
        })?;

        Ok(found)
    }

    /// How many objects `query` finds; see [`CollectionReader::count`].
    pub fn count(&self, query: &Query) -> Result<u64> {
        self.untyped().count(query)
    }
}

impl<T: CollectionType> TypedWriter<'_, T> {
    /// The objects `query` finds, with this transaction's changes; see
    /// [`TypedReader::query`].
    pub fn query(&self, query: &Query) -> Result<Vec<T>> {
        let (source, schema, collection) = self.untyped_ref().parts();
        let mut found = Vec::new();

        each_stored(source, schema, collection, query, &mut |record| {
            found.push(self.read(record)?);

            Ok(())
        })?;

        Ok(found)
    }

    /// How many objects `query` finds, with this transaction's changes.
    pub fn count(&self, query: &Query) -> Result<u64> {
        self.untyped_ref().count(query)
    }
}
