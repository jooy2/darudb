//! Building a query in Rust: [`Query`] and [`Filter`].

use std::ops::Not;

use super::ir::{Expr, Ir, Op};
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
}
