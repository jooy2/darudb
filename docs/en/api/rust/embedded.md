---
title: Embedded
order: 11
counterpart: /api/node/t
---

# Embedded

`Embedded` declares the fields of an embedded object, an object stored inside another object with fields of its own.

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Embedded
```

`Embedded::new` starts one, each method adds a field and returns it, and [`Type::object`](../../types/rust/type.md) makes it the type of a field. Its fields follow the rules of a [`Collection`](./collection.md)'s, and may hold links, lists of scalars and other embedded objects. An embedded object has no primary key and no index of its own, but a query reaches its fields with a path such as `address.city`.

A field whose type is an embedded object is declared with `field` or `optional`. It cannot have a default; its fields have theirs. [`CollectionWriter::update`](./collection-writer.md#update) replaces an embedded object whole.

```rust
use darudb::{Collection, Embedded, Schema, Type};

fn schema() -> Schema {
    let address = Embedded::new()
        .field("city", Type::String)
        .optional("street", Type::String)
        .with_default("floor", Type::Int, 0);

    Schema::new(1).collection(
        Collection::new("places")
            .primary_key("code", Type::String)
            .field("name", Type::String)
            .optional("address", Type::object(address)),
    )
}
```

## Associated functions

### of

```rust
pub fn of<E: EmbeddedType>() -> Self
```

The fields `E` declares, as [`#[derive(Embedded)]`](./derive.md#embedded) declares them. A field whose Rust type is `E` declares this for itself, so it is rarely needed.

### new

```rust
pub fn new() -> Self
```

An embedded object with no field yet. It is the same as `Embedded::default()`.

## Methods

### field

```rust
pub fn field(mut self, name: impl Into<String>, kind: Type) -> Self
```

Adds a required field.

### optional

```rust
pub fn optional(mut self, name: impl Into<String>, kind: Type) -> Self
```

Adds an optional field, which may be null.

### with_default

```rust
pub fn with_default(
    mut self,
    name: impl Into<String>,
    kind: Type,
    value: impl Into<Value>,
) -> Self
```

Adds a required field that holds `value` when the embedded object leaves it out.
