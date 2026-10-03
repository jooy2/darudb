---
title: Filter
order: 11
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

A value has the field's type. An `Int` field compares with an integer and never with a float, a `Float` field with any number, and a link with the linked collection's key. Every condition on a null field is false, except `is_null`. Strings compare by their bytes.

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
