---
title: Migration
order: 12
counterpart: /types/node/migration
---

# Migration

`Migration` says what schema version `n` changes from version `n - 1` beyond what the engine works out by itself: renames, replaced fields, deleted collections, and a function that moves data.

```rust
#[derive(Clone)]
pub struct Migration
```

[`OpenOptions::migration`](./open-options.md#migration) adds one. Opening a file that holds an older version of the schema runs the migrations from the version after the file's up to the declared one, in version order, in one write transaction together with the changes the engine makes by itself. Either the whole migration commits, or the file keeps its old schema and data and `open` fails. `Migration` implements `Debug`, `Send` and `Sync`.

The engine makes some changes by itself: a new collection, a new optional field or one with a default, a removed field, and a new or removed index. A version that changes nothing else needs no `Migration`. A renamed collection or field, a field whose type changes, and a collection that leaves the schema need one. Without it, a collection that is gone or a field whose type changed fails `open` with `INVALID_ARGUMENT`, and a renamed field reads as a removed field and a new one, which leaves its values behind. [Migrations](../../guide/migrations.md) explains each change.

```rust
use darudb::{Collection, Migration, OpenOptions, Schema, Type};

fn main() -> darudb::Result<()> {
    let v2 = Schema::new(2).collection(
        Collection::new("people")
            .field("full_name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::String, "")
            .unique("email"),
    );
    let migration = Migration::to(2)
        .rename_collection("users", "people")
        .rename_field("users", "name", "full_name")
        .replace_field("users", "age")
        .delete_collection("posts")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let before = migrating.previous("users", key.clone())?;
                let age = before
                    .and_then(|user| user.get("age")?.as_int())
                    .unwrap_or(0);
                let mut people = migrating.collection("people")?;

                if let Some(mut person) = people.get(key)? {
                    person.set("age", format!("{age} years"));
                    people.put(person)?;
                }
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

A migration names collections and fields as the schema before it named them, and applies its changes in this order whatever order they were given in: field renames, replaced fields, deleted collections, then collection renames. So `rename_field`, `replace_field` and `delete_collection` take a collection's old name even when the same migration renames it. Naming a collection or field the schema before it does not have, or renaming one to a name that is taken, fails `open` with `INVALID_ARGUMENT`, and so do two migrations to one version.

## Associated functions

### to

```rust
pub fn to(version: u64) -> Self
```

The migration to schema version `version`, from the one before. The version is from 2 up to the declared schema's; any other fails `open` with `INVALID_ARGUMENT`.

## Methods

### rename_collection

```rust
pub fn rename_collection(mut self, from: impl Into<String>, to: impl Into<String>) -> Self
```

Renames collection `from` to `to`. Its objects stay where they are, so a rename costs nothing however many objects there are.

### rename_field

```rust
pub fn rename_field(
    mut self,
    collection: impl Into<String>,
    from: impl Into<String>,
    to: impl Into<String>,
) -> Self
```

Renames field `from` of `collection` to `to`, using the collection's name before any rename this migration makes. No object is rewritten.

### replace_field

```rust
pub fn replace_field(
    mut self,
    collection: impl Into<String>,
    field: impl Into<String>,
) -> Self
```

Replaces field `field` of `collection` with a new field of the same name, as when its type changes. Like any new field of a collection the file already holds, the new one has to be optional or have a default, since no object holds a value for it yet. The old values stay readable in the migration's function through [`Migrating::previous`](./migrating.md#previous). A primary key cannot be replaced.

### delete_collection

```rust
pub fn delete_collection(mut self, name: impl Into<String>) -> Self
```

Deletes collection `name` with its objects and indexes. It goes at the end of the migration, so the migration's function can still read its objects through [`Migrating::previous`](./migrating.md#previous).

### run

```rust
pub fn run(
    mut self,
    function: impl Fn(&mut Migrating<'_>) -> Result<()> + Send + Sync + 'static,
) -> Self
```

Runs `function` as part of the migration, after the schema has become the new one, in the same write transaction. It gets a [`Migrating`](./migrating.md), which reads objects as the old schema did and writes them under the new one. An error it returns ends the migration, and the file keeps its old schema and data; `open` fails with that error. A function that fails for a reason of the application's own returns `Error::MigrationFailed`, whose code is `MIGRATION_FAILED`; see [`Error`](../../types/rust/error.md).
