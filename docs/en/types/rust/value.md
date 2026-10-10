---
title: Value
order: 2
group: objects
pageClass: reference-page
---

# Value

`Value` is the value of one field of an object, and also the type of primary keys and of the values a query compares fields with.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Value
```

An [`Object`](./object.md) holds a `Value` for each field. The calls that take a key, such as [`get`](../../api/rust/collection-reader.md) and [`delete`](../../api/rust/collection-writer.md), take `impl Into<Value>`, so a key can be passed as `42` or `"alice"`, and `insert` returns the key it stored as a `Value`. [`Filter`](../../api/rust/filter.md) takes its values the same way, and [`Query::parse`](../../api/rust/query.md) takes its parameters as a `&[Value]`. A value carries no field type of its own: the schema says whether an `Int` is a number, a link or a date. There is no date type, and a date or a time is an `Int` in a unit the application chooses.

## Variants

| Variant            | Holds                                |
| ------------------ | ------------------------------------ |
| `Null`             | No value: an optional field left out |
| `Bool(bool)`       | `false` or `true`                    |
| `Int(i64)`         | A signed 64-bit integer              |
| `Float(f64)`       | A 64-bit floating-point number       |
| `String(String)`   | UTF-8 text                           |
| `Bytes(Vec<u8>)`   | Any bytes                            |
| `List(Vec<Value>)` | A list of values of one type         |
| `Object(Object)`   | An embedded object                   |

## Field types

Each [`Type`](./type.md) a field can have takes one variant.

| Field type | Value |
| --- | --- |
| `Type::Bool` | `Bool` |
| `Type::Int` | `Int` |
| `Type::Float` | `Float` |
| `Type::String` | `String` |
| `Type::Bytes` | `Bytes` |
| `Type::Link` | The linked object's primary key: `Int`, `String` or `Bytes`, as that collection's key is |
| `Type::List` | `List`, whose elements each take the element type's variant |
| `Type::Object` | `Object`, whose fields follow these same rules |

A write checks every value against the schema and fails with `INVALID_ARGUMENT`, changing nothing, when one does not fit:

- **The variant has to match exactly.** An `Int` is not accepted in a `Float` field, nor a `Float` in an `Int` field.
- **`Null`** in an optional field is the same as leaving the field out. In a required field, `Null` or a missing field takes the field's default, and fails when the field has none, except the `id` of a collection keyed by an auto-increment, which gets the next number. A list cannot hold `Null` among its elements.
- **A link** is checked only for the type of the linked collection's key, not for whether that object exists. A collection keyed by an auto-increment has `Int` keys.
- **A name that is not a field** of the collection, or of the embedded object, is refused.

Reading gives the same variants back. An optional field without a value reads as `Null`, and a field the stored object predates reads as its default, or as `Null`.

A query is the one place where an `Int` stands for a `Float`: compared with a `Float` field, an integer from -2^53 to 2^53 means the float it equals, since a language with one type of number cannot tell `1` from `1.0`. [Queries](../../guide/queries.md) has the rest of how values compare.

## Methods

| Method                    | Returns            | For a value that is |
| ------------------------- | ------------------ | ------------------- |
| [`is_null`](#is-null)     | `bool`             | `Null`              |
| [`as_bool`](#as-bool)     | `Option<bool>`     | `Bool`              |
| [`as_int`](#as-int)       | `Option<i64>`      | `Int`               |
| [`as_float`](#as-float)   | `Option<f64>`      | `Float`             |
| [`as_str`](#as-str)       | `Option<&str>`     | `String`            |
| [`as_bytes`](#as-bytes)   | `Option<&[u8]>`    | `Bytes`             |
| [`as_list`](#as-list)     | `Option<&[Value]>` | `List`              |
| [`as_object`](#as-object) | `Option<&Object>`  | `Object`            |

None of these converts: `as_float` on an `Int` is `None`.

### is_null

```rust
pub fn is_null(&self) -> bool
```

Whether the value is `Value::Null`.

### as_bool

```rust
pub fn as_bool(&self) -> Option<bool>
```

The boolean, if the value is a `Bool`.

### as_int

```rust
pub fn as_int(&self) -> Option<i64>
```

The integer, if the value is an `Int`.

### as_float

```rust
pub fn as_float(&self) -> Option<f64>
```

The number, if the value is a `Float`.

### as_str

```rust
pub fn as_str(&self) -> Option<&str>
```

The text, if the value is a `String`.

### as_bytes

```rust
pub fn as_bytes(&self) -> Option<&[u8]>
```

The bytes, if the value is `Bytes`.

### as_list

```rust
pub fn as_list(&self) -> Option<&[Value]>
```

The elements, if the value is a `List`.

### as_object

```rust
pub fn as_object(&self) -> Option<&Object>
```

The embedded object, if the value is an `Object`.

## Conversions

`Value` implements `From` for these types, which is what lets `Object::with`, `Object::set` and every call taking `impl Into<Value>` accept plain Rust values.

| From                               | Variant                               |
| ---------------------------------- | ------------------------------------- |
| `bool`                             | `Bool`                                |
| `i64`, `i32`, `u32`                | `Int`                                 |
| `f64`                              | `Float`                               |
| `&str`, `String`                   | `String`                              |
| `&[u8]`, `Vec<u8>`                 | `Bytes`                               |
| `Vec<Value>`                       | `List`                                |
| `Object`                           | `Object`                              |
| `Option<T>` where `T: Into<Value>` | `Null` for `None`, else `T`'s variant |

- There is no conversion from `u64`, `usize` or `f32`. A `u64` can exceed what an `Int` holds, so convert it with `i64::try_from` first.
- A byte string literal such as `b"abc"` is an array, not a slice: pass `b"abc".as_slice()` or `b"abc".to_vec()`.
- A list is built from values: `vec![Value::from("a"), Value::from("b")]`, not `vec!["a", "b"]`.

```rust
use darudb::{Object, Value};

let post = Object::new()
    .with("title", "Hello")
    .with("views", 0)
    .with("rating", 4.5)
    .with("cover", b"\x89PNG".to_vec())
    .with("subtitle", None::<String>)
    .with("tags", vec![Value::from("intro"), Value::from("rust")]);
```
