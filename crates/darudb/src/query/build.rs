//! Building a query in Rust: [`Query`] and [`Filter`].

use std::ops::Not;

use super::ir::{Expr, Ir, Op};
use crate::error::Result;
use crate::format::object::Value;

/// A path to a field: its name, or names joined by `.` through embedded
/// objects and links, such as `address.city` or `author.name`.
fn path(text: &str) -> Vec<String> {
    text.split('.').map(str::to_owned).collect()
}

/// A condition that objects have to meet, for [`Query::filter`].
///
/// A condition on a list holds when it holds for any element, and one on a
/// field that is null is false, except [`is_null`](Self::is_null). A value
/// has to have the field's type: an `int` field compares with an `int`, a
/// link with the linked collection's key.
///
/// ```
/// use darudb::Filter;
///
/// let adults_named_a = Filter::ge("age", 18).and(Filter::starts_with("name", "A"));
/// let not_in_seoul = !Filter::eq("address.city", "Seoul");
/// # let _ = (adults_named_a, not_in_seoul);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Filter(pub(crate) Expr);

impl Filter {
    fn test(op: Op, field: &str, values: Vec<Value>) -> Self {
        Filter(Expr::test(op, path(field), values))
    }

    /// The field equals `value`. Equal to null means [`is_null`](Self::is_null).
    pub fn eq(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Eq, field, vec![value.into()])
    }

    /// The field is not null and differs from `value`. Different from null
    /// means [`is_not_null`](Self::is_not_null).
    pub fn ne(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Ne, field, vec![value.into()])
    }

    /// The field is less than `value`.
    pub fn lt(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Lt, field, vec![value.into()])
    }

    /// The field is at most `value`.
    pub fn le(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Le, field, vec![value.into()])
    }

    /// The field is greater than `value`.
    pub fn gt(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Gt, field, vec![value.into()])
    }

    /// The field is at least `value`.
    pub fn ge(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Ge, field, vec![value.into()])
    }

    /// The field lies from `low` to `high`, both included.
    pub fn between(field: &str, low: impl Into<Value>, high: impl Into<Value>) -> Self {
        Self::test(Op::Between, field, vec![low.into(), high.into()])
    }

    /// The field equals one of `values`.
    pub fn is_in<V: Into<Value>>(field: &str, values: impl IntoIterator<Item = V>) -> Self {
        Self::test(Op::In, field, values.into_iter().map(Into::into).collect())
    }

    /// A string field contains `value`, or a list field holds the element
    /// `value`.
    pub fn contains(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::Contains, field, vec![value.into()])
    }

    /// A string field starts with `value`.
    pub fn starts_with(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::StartsWith, field, vec![value.into()])
    }

    /// A string field ends with `value`.
    pub fn ends_with(field: &str, value: impl Into<Value>) -> Self {
        Self::test(Op::EndsWith, field, vec![value.into()])
    }

    /// The field is null.
    pub fn is_null(field: &str) -> Self {
        Self::test(Op::IsNull, field, Vec::new())
    }

    /// The field is not null.
    pub fn is_not_null(field: &str) -> Self {
        !Self::is_null(field)
    }

    /// Both this condition and `other` hold.
    #[must_use]
    pub fn and(self, other: Filter) -> Self {
        Filter(Expr::and([self.0, other.0]))
    }

    /// This condition or `other` holds, or both.
    #[must_use]
    pub fn or(self, other: Filter) -> Self {
        Filter(Expr::or([self.0, other.0]))
    }
}

impl Not for Filter {
    type Output = Filter;

    /// The condition does not hold.
    fn not(self) -> Filter {
        Filter(Expr::Not(Box::new(self.0)))
    }
}

/// A query as the IR carries it across the language boundary: the query, the
/// collection it names, and whether it counts the objects rather than
/// returning them (`design/objects.md`, "The IR").
///
/// A language binding builds the IR in its own language and hands the bytes
/// to the engine, which reads them with [`decode`](Self::decode). Rust code
/// has no need of it and uses [`Query`] directly.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRequest {
    /// The collection the query runs on.
    pub collection: String,
    /// What to find, in what order, and how many.
    pub query: Query,
    /// Whether to count the objects rather than return them.
    pub count: bool,
}

impl QueryRequest {
    /// Reads the IR in `bytes`. IR that does not decode, or that names an
    /// unknown operator or leaves out what one uses, is
    /// [`Error::InvalidQuery`](crate::Error::InvalidQuery).
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let (collection, ir, count) = super::ir::decode(bytes)?;

        Ok(Self {
            collection,
            query: Query { ir },
            count,
        })
    }

    /// The IR of this request.
    pub fn encode(&self) -> Result<Vec<u8>> {
        super::ir::encode(&self.collection, &self.query.ir, self.count)
    }
}

/// What to find in a collection, in what order, and how many, for
/// [`CollectionReader::query`](crate::CollectionReader::query) and
/// [`CollectionReader::count`](crate::CollectionReader::count).
///
/// Without a sort, objects come in primary key order. Objects that sort
/// equal come in primary key order too.
///
/// ```
/// use darudb::{Filter, Query};
///
/// let query = Query::new()
///     .filter(Filter::ge("age", 18))
///     .sort_by_desc("age")
///     .limit(10);
/// # let _ = query;
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Query {
    pub(crate) ir: Ir,
}

impl Query {
    /// Every object of the collection, in primary key order.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps only the objects that meet `filter`, and those of any filter
    /// given before.
    #[must_use]
    pub fn filter(mut self, filter: Filter) -> Self {
        self.ir.filter = Some(match self.ir.filter.take() {
            Some(before) => Expr::and([before, filter.0]),
            None => filter.0,
        });
        self
    }

    /// Sorts by `field` in ascending order, after any sort given before. Null
    /// sorts first.
    #[must_use]
    pub fn sort_by(mut self, field: &str) -> Self {
        self.ir.sort.push((path(field), false));
        self
    }

    /// Sorts by `field` in descending order, after any sort given before.
    /// Null sorts last.
    #[must_use]
    pub fn sort_by_desc(mut self, field: &str) -> Self {
        self.ir.sort.push((path(field), true));
        self
    }

    /// Skips the first `count` objects of the result.
    #[must_use]
    pub fn offset(mut self, count: u64) -> Self {
        self.ir.offset = count;
        self
    }

    /// Returns at most `count` objects.
    #[must_use]
    pub fn limit(mut self, count: u64) -> Self {
        self.ir.limit = Some(count);
        self
    }

    /// Returns at most the first object: a limit of one, or of none if the
    /// query's own limit is zero. A lookup of one object stops reading there.
    #[must_use]
    pub fn first(mut self) -> Self {
        self.ir.limit = Some(self.ir.limit.map_or(1, |limit| limit.min(1)));
        self
    }

    /// Parses `text` in the query language, with `parameters` for `$0`, `$1`
    /// and on. It builds the same query the builder methods would.
    ///
    /// A filter comes first, then `SORT BY`, `LIMIT` and `OFFSET`, each
    /// optional. Keywords are case-insensitive, strings are in double
    /// quotes, and a name that is a keyword or not a plain word goes in
    /// backticks. A value that comes from outside the application belongs in
    /// a parameter, never in the text. Text that does not parse fails with
    /// [`Error::InvalidQuery`](crate::Error::InvalidQuery), naming the
    /// character where it went wrong.
    ///
    /// ```
    /// use darudb::Query;
    ///
    /// let query = Query::parse(
    ///     r#"age >= $0 AND (name STARTSWITH "A" OR tags CONTAINS "admin")
    ///        SORT BY age DESC LIMIT 10"#,
    ///     &[18.into()],
    /// )?;
    /// # let _ = query;
    /// # Ok::<(), darudb::Error>(())
    /// ```
    pub fn parse(text: &str, parameters: &[Value]) -> Result<Self> {
        Ok(Self {
            ir: super::parse::parse(text, Some(parameters))?,
        })
    }

    /// Parses `text` in the query language as [`parse`](Self::parse) does,
    /// keeping `$0`, `$1` and on as parameters, for a query that runs many
    /// times with different values: [`bind`](Self::bind) gives it values
    /// without parsing it again. A query with a parameter that has no value
    /// fails with [`Error::InvalidQuery`](crate::Error::InvalidQuery) when it
    /// runs.
    ///
    /// ```
    /// use darudb::Query;
    ///
    /// let by_email = Query::prepare("email == $0")?;
    /// let query = by_email.bind(&["alice@example.com".into()])?;
    /// # let _ = query;
    /// # Ok::<(), darudb::Error>(())
    /// ```
    pub fn prepare(text: &str) -> Result<Self> {
        Ok(Self {
            ir: super::parse::parse(text, None)?,
        })
    }

    /// This query with `parameters` for its `$0`, `$1` and on. A query without
    /// parameters comes back as it is.
    pub fn bind(&self, parameters: &[Value]) -> Result<Self> {
        Ok(Self {
            ir: self.ir.bind(parameters)?,
        })
    }
}
