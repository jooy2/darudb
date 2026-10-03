---
title: Migrating
order: 13
---

# Migrating

`Migrating` is the write transaction of a migration as a migration function gets it: collections of the new schema, and objects as the old schema read them.

```rust
#[derive(Debug)]
pub struct Migrating<'a>
```

A function given to [`Migration::run`](./migration.md#run) gets `&mut Migrating`. Its [`collection`](#collection) reaches the collections of the declared schema, even in a step of a migration that spans several versions. [`previous`](#previous) and [`previous_keys`](#previous-keys) read the objects as the schema the file held before the whole migration named them, with the values of fields the migration removed or replaced, until the migration commits. The engine commits the transaction once every function has returned.

Writing an object keeps only the fields of the new schema, so a function reads an object through `previous` before it writes it.

```rust
use darudb::{Collection, Migration, Object, OpenOptions, Schema, Type};

fn main() -> darudb::Result<()> {
    let v2 = Schema::new(2).collection(
        Collection::new("users")
            .field("name", Type::String)
            .with_default("age", Type::String, ""),
    );
    let migration = Migration::to(2)
        .replace_field("users", "age")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let age = migrating
                    .previous("users", key.clone())?
                    .and_then(|user| user.get("age")?.as_int())
                    .unwrap_or(0);
                let mut users = migrating.collection("users")?;

                users.update(key, Object::new().with("age", format!("{age} years")))?;
            }

            Ok(())
        });
    let db = OpenOptions::new()
        .schema(v2)
        .migration(migration)
        .open("app.darudb")?;

    db.close()
}
```

## Methods

### previous_version

```rust
pub fn previous_version(&self) -> u64
```

The schema version the file held before the migration. It is the same in every step of a migration that spans several versions.

### collection

```rust
pub fn collection(&mut self, name: &str) -> Result<CollectionWriter<'_>>
```

Collection `name` of the new schema, for reading and writing its objects; see [`CollectionWriter`](./collection-writer.md). A collection the new schema does not have fails with `INVALID_ARGUMENT`.

### previous_keys

```rust
pub fn previous_keys(&self, collection: &str) -> Result<Vec<Value>>
```

The primary keys of every object of `collection`, named as the schema before the migration named it, in key order. A collection that schema does not have fails with `INVALID_ARGUMENT`.

### previous

```rust
pub fn previous(&self, collection: &str, key: impl Into<Value>) -> Result<Option<Object>>
```

The object of `collection` whose primary key is `key`, as the schema before the migration reads it: with the names it gave the collection and its fields, and the values of fields the migration removed or replaced. A collection the migration deletes can still be read this way until the migration commits.

It reads the object as it is now. Once the function has written an object, the fields the migration dropped read as a record without them does, as their defaults or null, so read an object this way before writing it.

### transaction

```rust
pub fn transaction(&mut self) -> &mut WriteTransaction
```

The write transaction the migration runs in, for reading and writing the storage kernel's trees beside the collections. Committing it is the engine's job, once every migration function has returned.

### previous_record

```rust
pub fn previous_record(
    &self,
    collection: &str,
    key: impl Into<Value>,
) -> Result<Option<Vec<u8>>>
```

The record of the object `previous` reads, as the file holds it, for a language binding that decodes records itself with [`PendingMigration::previous_schema_record`](./opening.md#previous-schema-record). A field the record lacks reads as its default or null, required or not.
