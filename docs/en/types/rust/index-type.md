---
title: Index
order: 14.5
group: objects
counterpart: /types/node/stored-schema
pageClass: reference-page
---

# Index

`Index` is an index of a collection, as a schema declares it, read back: the fields it is on, in order, and whether it is unique.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index
```

[`Collection::index`](../../api/rust/collection.md#index) and [`Collection::unique`](../../api/rust/collection.md#unique) declare an index on one field, and [`Collection::index_on`](../../api/rust/collection.md#index-on) and [`Collection::unique_on`](../../api/rust/collection.md#unique-on) one on several fields or on a field inside an embedded object. [`Collection::indexes`](../../api/rust/collection.md#indexes) gives them back, as it does for the schema [`Database::stored_schema`](../../api/rust/database.md#stored-schema) reads from a file.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;

    for collection in db.stored_schema().iter().flat_map(|schema| schema.collections()) {
        for index in collection.indexes() {
            let fields: Vec<String> = index.fields().collect();
            let unique = if index.is_unique() { " (unique)" } else { "" };

            println!("{}: {}{unique}", collection.name(), fields.join(", "));
        }
    }

    Ok(())
}
```

## Methods

### fields

```rust
pub fn fields(&self) -> impl ExactSizeIterator<Item = String> + '_
```

The fields the index is on, in the order its entries sort by them. Each is a field's name, or names joined by `.` through embedded objects, such as `address.city`.

### is_unique

```rust
pub fn is_unique(&self) -> bool
```

Whether two objects may not hold the same values in every field of the index. Any number of objects may hold null in one of them.
