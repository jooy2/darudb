---
title: Collection
order: 8
---

# Collection

`Collection` declares one collection of a schema: its fields, its primary key and its indexes.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Collection
```

`Collection::new` starts one, each method adds a field or an index and returns the collection, and [`Schema::collection`](./schema.md#collection) adds it to a schema. Field types are [`Type`](../../types/rust/type.md)s. Without [`primary_key`](#primary-key), the collection is keyed by an `Int` field called `id` that the engine assigns in increasing order.

```rust
use darudb::{Collection, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}
```

The declaration is checked when a file is opened with it. Each of these fails `open` with `INVALID_ARGUMENT`:

- **Names.** A collection or field with an empty name, or two fields with one name. A field called `id` in a collection without a primary key, since `id` is the auto-increment's.
- **Primary key.** A primary key that is not an `Int`, a `String` or `Bytes`.
- **Types.** A link to a collection the schema does not have, or a list of lists or of embedded objects.
- **Defaults.** A default that does not have the field's type, or one on a link or an embedded object.
- **Indexes.** An index on a field the collection does not have, on an embedded object or a list of them, or on one field twice.

## Associated functions

### new

```rust
pub fn new(name: impl Into<String>) -> Self
```

A collection called `name` with no field yet, keyed by an auto-increment.

## Methods

### primary_key

```rust
pub fn primary_key(mut self, name: impl Into<String>, kind: Type) -> Self
```

Adds field `name` of type `kind`, an `Int`, a `String` or `Bytes`, as the primary key. The key is required, and a migration cannot change it later.

### field

```rust
pub fn field(mut self, name: impl Into<String>, kind: Type) -> Self
```

Adds a required field. Writing an object without it fails with `INVALID_ARGUMENT`.

### optional

```rust
pub fn optional(mut self, name: impl Into<String>, kind: Type) -> Self
```

Adds an optional field, which may be null and is null when an object leaves it out.

### with_default

```rust
pub fn with_default(
    mut self,
    name: impl Into<String>,
    kind: Type,
    value: impl Into<Value>,
) -> Self
```

Adds a required field that holds `value` when an object leaves it out. A default can be a scalar or a list of scalars of the field's type. An object written before a field existed reads its default, so a later version of the schema has to keep a default once the field has one.

### index

```rust
pub fn index(mut self, name: impl Into<String>) -> Self
```

Indexes field `name`, so that queries with a condition on it, or sorted by it alone, read the index rather than every object. The field holds a scalar, a link or a list of them; an index on a list has an entry for each element.

### unique

```rust
pub fn unique(mut self, name: impl Into<String>) -> Self
```

Indexes field `name` and keeps its values unique: two objects cannot hold the same value, though any number may hold null. A write that would take a value another object holds fails with `DUPLICATE_KEY`, and so does opening a file whose objects hold one value twice when a new version adds the unique index.
