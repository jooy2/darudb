---
title: Schema
order: 9
---

# Schema

`Schema` declares the collections of a database and what their objects hold, at one version.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Schema
```

An application builds its schema at run time with `Schema::new` and [`Collection`](./collection.md), and opens the file with it through [`OpenOptions::schema`](./open-options.md#schema). The first open stores the schema in the file. Every later open compares the declared schema with the stored one: the same version with a different schema fails with `SCHEMA_MISMATCH`, an older stored version is migrated, and a newer one fails with `SCHEMA_TOO_NEW`. Declaring collections or indexes in another order is not a change.

A schema is checked when a file is opened with it, not while it is built. One that cannot be stored fails `open` with `INVALID_ARGUMENT` before any file is made: [`Collection`](./collection.md) lists the rules for each collection. [Collections and objects](../../guide/objects.md) shows a schema in use, and [Migrations](../../guide/migrations.md) what a new version may change.

```rust
use darudb::{Collection, OpenOptions, Schema, Type};

fn main() -> darudb::Result<()> {
    let schema = Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .field("title", Type::String)
                .field("author", Type::link("users"))
                .index("author"),
        );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    db.close()
}
```

## Associated functions

### new

```rust
pub fn new(version: u64) -> Self
```

A schema at `version` with no collection yet. Versions start at 1, and an application raises the version whenever it changes the schema; version 0 fails `open` with `INVALID_ARGUMENT`.

### decode

```rust
pub fn decode(bytes: &[u8]) -> Result<Self>
```

Reads a schema that another language declared, encoded the way the file stores a schema, with ids of the encoder's choosing. A language binding builds its schema this way; a Rust program uses `new`. The ids only tie links and indexes to what they name, and the file gives the collections and fields ids of its own. A record that does not decode, or whose ids are inconsistent, fails with `INVALID_ARGUMENT`, and the schema is checked like any other when a file is opened with it. [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-stored-schema) specifies the encoding.

## Methods

### collection

```rust
pub fn collection(mut self, collection: Collection) -> Self
```

Adds `collection`. Two collections with one name fail `open` with `INVALID_ARGUMENT`.
