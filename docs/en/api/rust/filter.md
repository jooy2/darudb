---
title: Filter
order: 14
counterpart: /api/node/conditions
---

# Filter

`Filter` is a condition that objects have to meet, for [`Query::filter`](./query.md#filter).

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Filter
```

Each associated function makes a condition on one field, and `and`, `or` and `!` combine conditions. Building a filter never fails: it is checked against the collection's schema when the query runs, and a field that is not there, or a value of the wrong type, fails with `INVALID_QUERY` then. Every condition has a form in the query language, which [`Query::parse`](./query.md#parse) lists.

```rust
use darudb::Filter;

fn main() {
    let adults_named_a = Filter::ge("age", 18).and(Filter::starts_with("name", "A"));
    let not_in_seoul = !Filter::eq("address.city", "Seoul");
    let tagged = Filter::is_in("tags", ["intro", "news"]).or(Filter::is_null("tags"));
    let _ = (adults_named_a, not_in_seoul, tagged);
}
```

## Paths

The `field` of a condition is a path: a field's name, or names joined by `.` through embedded objects and links.

- **Embedded objects.** `address.city` tests a field of the embedded object in `address`. A path cannot end at an embedded object, since a condition tests one of its fields.
- **Links.** `author.name` tests a field of the object the link `author` points to, and `author` alone tests the linked object's primary key. A link to an object that is not there reads as null.
- **Lists.** A condition on a list holds when it holds for any element, and `contains` on a list looks for an element. An empty list is not null.
- **Length.** A path has at most 32 names.

## Values

A value has the field's type. An `Int` field compares with an integer and never with a float, a `Float` field with a float or with an integer from -2^53 to 2^53, which means the float it equals, and a link with the linked collection's key. Every condition on a null field is false, except `is_null`. Strings compare by their bytes.

## Associated functions

### eq

```rust
pub fn eq(field: &str, value: impl Into<Value>) -> Self
```

The field equals `value`. Equal to `Value::Null` means [`is_null`](#is-null).

### ne

```rust
pub fn ne(field: &str, value: impl Into<Value>) -> Self
```

The field is not null and differs from `value`. Different from `Value::Null` means [`is_not_null`](#is-not-null).

### lt

```rust
pub fn lt(field: &str, value: impl Into<Value>) -> Self
```

The field is less than `value`.

### le

```rust
pub fn le(field: &str, value: impl Into<Value>) -> Self
```

The field is at most `value`.

### gt

```rust
pub fn gt(field: &str, value: impl Into<Value>) -> Self
```

The field is greater than `value`.

### ge

```rust
pub fn ge(field: &str, value: impl Into<Value>) -> Self
```

The field is at least `value`.

### between

```rust
pub fn between(field: &str, low: impl Into<Value>, high: impl Into<Value>) -> Self
```

The field lies from `low` to `high`, both included.

### is_in

```rust
pub fn is_in<V: Into<Value>>(field: &str, values: impl IntoIterator<Item = V>) -> Self
```

The field equals one of `values`.

### contains

```rust
pub fn contains(field: &str, value: impl Into<Value>) -> Self
```

A string field contains `value`, or a list field holds the element `value`.

### starts_with

```rust
pub fn starts_with(field: &str, value: impl Into<Value>) -> Self
```

A string field starts with `value`.

### ends_with

```rust
pub fn ends_with(field: &str, value: impl Into<Value>) -> Self
```

A string field ends with `value`.

### is_null

```rust
pub fn is_null(field: &str) -> Self
```

The field is null.

### is_not_null

```rust
pub fn is_not_null(field: &str) -> Self
```

The field is not null. It is `!Filter::is_null(field)`.

### linked

```rust
pub fn linked(collection: &str, link: &str) -> Self
```

Holds for an object that some object of `collection` links to through its link field `link`, a name or a path such as `meta.author`, to the collection the query runs on: `Filter::linked("posts", "author")` finds the users with a post, and `!` of it those without. The link may be a list of links. A collection that is not there, or a field that is not such a link, fails with `INVALID_QUERY` when the query runs. It is `ANY posts.author` in the [query language](../../guide/query-language.md#objects-that-link-here).

### linked_where

```rust
pub fn linked_where(collection: &str, link: &str, filter: Filter) -> Self
```

[`linked`](#linked), with `filter` on the fields of `collection`, which one linking object has to meet whole: `Filter::linked_where("posts", "author", Filter::ge("year", 2020))`. It is `ANY posts.author (year >= 2020)`.

## Methods

### and

```rust
pub fn and(self, other: Filter) -> Self
```

Both this condition and `other` hold.

### or

```rust
pub fn or(self, other: Filter) -> Self
```

This condition or `other` holds, or both.

### not

```rust
fn not(self) -> Filter
```

The condition does not hold. It is the `Not` operator, so it is written `!filter`.

### ignore_case

```rust
pub fn ignore_case(self) -> Self
```

This filter with every test of `eq`, `ne`, `is_in`, `contains`, `starts_with` and `ends_with` in it comparing strings ignoring case, by Unicode's simple case folding; the other tests stay as they are. `Filter::eq("name", "ada").ignore_case()` finds `Ada` and `ADA`. Such a test of a field that holds no strings fails with `INVALID_QUERY` when the query runs. It is `IGNORE CASE` in the [query language](../../guide/query-language.md#ignore-case), and no index answers it.
