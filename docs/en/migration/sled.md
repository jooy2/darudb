---
title: sled
order: 6
languages: [rust]
---

# Migrating from sled

This page moves the data of a sled database into DaruDB: how trees, keys and values map onto collections, objects and fields, a program that copies every entry, and what changes in the application.

## How the two map

| sled | DaruDB |
| --- | --- |
| A database: a folder of files | A database: one file |
| A tree whose values are records | A collection |
| A tree the application keeps as an index, from a field's value to a key | A declared index or unique index, which the engine keeps in step with the objects |
| A key, bytes | The primary key, of type integer, string or bytes |
| A value, bytes | An object, decoded from its bytes the way the application wrote them |
| A transaction over several trees | A write transaction, over any collections |
| `flush` | A sync commit, or `sync` after deferred commits |
| Writes between two flushes | Deferred commits |
| A merge operator | No counterpart: read the object, change it, and write it back in one transaction |
| `watch_prefix`, a subscriber | No counterpart: read again after a write |

- **The schema comes from the application.** sled stores bytes and knows nothing of their shape, so the schema is declared from what the application writes into each tree. The engine checks every value against its field's type, and the copy stops with `INVALID_ARGUMENT` at the first that does not fit.
- **Keys carry over.** An integer key stored big-endian, so that it sorts as a number, becomes an integer key; a key of UTF-8 text becomes a string key; any other key can stay bytes. A key the application built from parts becomes a field of each part and an index, or a string key.
- **Indexes are declared, not copied.** A tree that maps an email to a user's key is the application's own index. Leave it out of the copy and declare a unique index on `email`: the engine fills it as the objects go in.

## Declare the schema

The example moves a database of three trees, as an application would have written them:

| Tree | Key | Value |
| --- | --- | --- |
| `users` | The user's id, a `u64`, big-endian | JSON: `name`, `email`, which may be `null`, and `age` |
| `posts` | The post's slug, UTF-8 | JSON: `author_id`, the key of a user, `body`, `published_at` and a list of `tags` |
| `users_by_email` | An email, UTF-8 | The key of the user who has it |

`users` keeps its integer key as the automatic `id`, `author_id` becomes a link, and `users_by_email` becomes a unique index:

```rust
use darudb::{Collection, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email")
                .index("age"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .optional("author", Type::link("users"))
                .field("body", Type::String)
                .optional("published_at", Type::String)
                .field("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}
```

## Copy the entries

The program reads the database with the sled release the application wrote it with, and decodes each value into a struct with serde:

```toml
[dependencies]
darudb = "1.3"
sled = "0.34"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

```rust
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use serde::Deserialize;

/// A user as the application stored it.
#[derive(Deserialize)]
struct User {
    name: String,
    email: Option<String>,
    age: i64,
}

/// A post as the application stored it.
#[derive(Deserialize)]
struct Post {
    author_id: Option<u64>,
    body: String,
    published_at: Option<String>,
    tags: Vec<String>,
}

/// Copies `objects` into collection `name`, a thousand to each deferred commit.
fn copy<I>(db: &Database, name: &str, objects: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = Result<Object, Box<dyn Error>>>,
{
    let mut objects = objects.into_iter().peekable();

    while objects.peek().is_some() {
        let mut txn = db.begin_write()?;
        let mut collection = txn.collection(name)?;

        for object in objects.by_ref().take(1000) {
            collection.insert(object?)?;
        }

        txn.commit_deferred()?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let sled = sled::open("app.sled")?;
    let users = sled.open_tree("users")?;
    let posts = sled.open_tree("posts")?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    copy(
        &db,
        "users",
        users.iter().map(|entry| -> Result<Object, Box<dyn Error>> {
            let (key, value) = entry?;
            let id = u64::from_be_bytes(key.as_ref().try_into()?);
            let user: User = serde_json::from_slice(&value)?;

            Ok(Object::new()
                .with("id", i64::try_from(id)?)
                .with("name", user.name)
                .with("email", user.email)
                .with("age", user.age))
        }),
    )?;

    copy(
        &db,
        "posts",
        posts.iter().map(|entry| -> Result<Object, Box<dyn Error>> {
            let (key, value) = entry?;
            let post: Post = serde_json::from_slice(&value)?;

            Ok(Object::new()
                .with("slug", std::str::from_utf8(&key)?)
                .with("author", post.author_id.map(i64::try_from).transpose()?)
                .with("body", post.body)
                .with("published_at", post.published_at)
                .with("tags", post.tags.into_iter().map(Value::from).collect::<Vec<_>>()))
        }),
    )?;

    // Every deferred commit is durable once this returns.
    db.close()?;

    Ok(())
}
```

sled has no read-only mode, and a database is open in one process at a time: close the application before the copy, and copy from a copy of the folder to be sure nothing changes the original. An integer key of DaruDB is signed, so a `u64` above `i64::MAX` stops the copy at `try_from`; such keys can stay bytes instead, `Type::Bytes` with the key as it is. Values written with bincode or another serde format are read with that crate in place of `serde_json`.

## Check the copy

Compare `users.len()` on each tree with `len` on each collection, and run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

What the application did with scans and its own index trees becomes a query, which the engine runs through an index when one fits:

| What the application did | DaruDB query language |
| --- | --- |
| Got the key from `users_by_email`, then the user from `users` | `email == $0`, on `users` |
| Scanned `users`, testing the age in the program | `age BETWEEN 18 AND 30` |
| Scanned `users` by a key prefix | `STARTSWITH` on a field of that part of the key |
| Scanned `users`, keeping the ten oldest | `SORT BY age DESC LIMIT 10` |
| Scanned `posts` for the ones with a tag | `tags CONTAINS "news"`, on `posts` |
| Read a post, then its author by key | `author.name == "Alice"`, on `posts`, for posts by the author's name |

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **No index upkeep.** Writing an object updates every index declared on its collection in the same transaction, so the code that kept `users_by_email` in step goes away.
- **No encoding.** Objects go in and come out as an `Object`, or as a struct with `#[derive(Object)]`, read straight from the record. See [Collections and objects](../guide/objects.md).
- **Durability is chosen per commit.** A sync commit, the default, is durable when it returns; a deferred one becomes durable at the next sync commit, `sync`, `close`, or when its window's limits pass. See [Transactions](../guide/transactions.md).
- **Several processes** can use one file at the same time: one writer, any number of readers. See [Several processes](../guide/processes.md).
- **No subscribers.** Nothing calls the application when data changes. After a write, read again what the application shows.
- **Encryption** of the whole file comes with the engine; see [Encryption](../guide/encryption.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
