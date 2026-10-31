---
title: Collection
order: 10
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
- **Types.** A link to a collection the schema does not have, or a list of lists.
- **Defaults.** A default that does not have the field's type, or one on a link or an embedded object.
- **Indexes.** An index on a field the collection does not have, on an embedded object or a list of them, on a path that goes on past a scalar or through a link, or on the same fields twice; an index on no field or on more than eight; an index on several fields with a path through a list.

## Associated functions

### of

```rust
pub fn of<T: CollectionType>() -> Self
```

The collection `T` declares, as [`#[derive(Object)]`](./derive.md) or a hand-written [`CollectionType`](../../types/rust/collection-type.md) declares it.

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

### index_on

```rust
pub fn index_on<P: Into<String>>(mut self, paths: impl IntoIterator<Item = P>) -> Self
```

Indexes the fields `paths`, in order, as one index, whose entries sort by the first field, then by the second, and so on. Each is a field's name, or names joined by `.` through embedded objects, such as `address.city`, and there are one to eight of them. `index_on(["address.city"])` indexes one field inside an embedded object, which `index` cannot, since `index("address.city")` names a field called `address.city`.

```rust
use darudb::{Collection, Embedded, Type};

fn posts() -> Collection {
    Collection::new("posts")
        .field("author", Type::String)
        .field("written", Type::Date)
        .field("place", Type::object(Embedded::new().field("city", Type::String)))
        .index_on(["author", "written"])
        .index_on(["place.city"])
}
```

A query reads the index when it tests the first fields for equality, with `==`, `IS NULL` or `IN`, and the next one, if any, for a range or with `STARTSWITH`. The index gives its objects in order when the query sorts by every field after those it tests for equality, in the index's order: `author == $0 SORT BY written DESC LIMIT 20` reads the twenty newest posts of one author and stops. A path may go through a list of embedded objects, such as `stops.city`, only in an index of that one field, which has an entry for each element. A file with an index on several fields, or on a field inside an embedded object, is in object format 2, which [`OpenOptions::upgrade_format`](./open-options.md#upgrade-format) governs.

### unique_on

```rust
pub fn unique_on<P: Into<String>>(mut self, paths: impl IntoIterator<Item = P>) -> Self
```

[`index_on`](#index-on), keeping the fields' values unique together: two objects cannot hold the same values in all of them, though any number may hold null in one of them. A write that would repeat another object's values fails with `DUPLICATE_KEY`, as with [`unique`](#unique).

### cascade

```rust
pub fn cascade(mut self, name: impl Into<String>) -> Self
```

Makes the link field `name`, declared before this call, cascade: deleting the object it names deletes the object that holds the link, in the same transaction, and so on down the chain. The field is a link to one object, of the collection itself; anything else, or a name not declared by then, fails with `INVALID_ARGUMENT` when the database opens. Give the field an [index](#index): without one that begins with it, each delete of an object it can name reads this whole collection. A file with a cascading link is in object format 2. The guide has [the rules](../../guide/objects.md#delete-what-links-to-an-object).

### name

```rust
pub fn name(&self) -> &str
```

The collection's name. This method and the four below read a collection back, as a program does with the schema [`Database::stored_schema`](./database.md#stored-schema) gives for a file it did not write.

### fields

```rust
pub fn fields(&self) -> &[Field]
```

The fields, as [`Field`](../../types/rust/field.md)s, in the order they were declared, or in the file's order for a schema read from a file. The primary key's field is one of them, unless the key is the auto-increment, whose `id` is not declared.

### key

```rust
pub fn key(&self) -> &str
```

The name of the primary key's field: `id` for the auto-increment.

### is_auto_increment

```rust
pub fn is_auto_increment(&self) -> bool
```

Whether the primary key is the auto-increment: an `int` field called `id`, which the engine assigns in increasing order.

### indexes

```rust
pub fn indexes(&self) -> &[Index]
```

The indexes, as [`Index`](../../types/rust/index-type.md)es, in the order they were declared, or in the file's order for a schema read from a file. An index declared with `index` or `unique` has one field.
