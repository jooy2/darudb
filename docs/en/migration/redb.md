---
title: redb
order: 5
languages: [rust]
---

# Migrating from redb

This page moves the data of a redb file into DaruDB: how tables, keys and values map onto collections, objects and fields, a program that copies every entry, and what changes in the application.

## How the two map

| redb | DaruDB |
| --- | --- |
| A database: one file | A database: one file |
| A table whose values are records | A collection |
| A table the application keeps as an index, from a field's value to a key | A declared index or unique index, which the engine keeps in step with the objects |
| A multimap table, several values under one key | A list field on the object, or a collection of its own |
| A key of `u64`, `i64`, `&str`, `&[u8]` | The primary key, of type integer, string or bytes |
| A key that is a tuple | A field of each part and an index, or a string key built from the parts |
| A value that is a tuple, or a type of the application's with `redb::Value` | An object, with a field for each part |
| `Option<T>` | An optional field |
| `Durability::Immediate` | A sync commit, the default |
| `Durability::None` | A deferred commit, which becomes durable at the next sync commit, `sync` or `close` |
| A savepoint | No counterpart: abort the write transaction instead |

- **The schema comes from the table definitions.** A redb table's key and value types are in the program, not in the file, so the DaruDB schema is declared from the `TableDefinition`s the application opens.
- **Keys carry over.** A key of DaruDB is a signed 64-bit integer, a string or bytes. A `u64` key fits as long as it is not above `i64::MAX`; past that, keep it as its eight bytes, big-endian, which sort the same way.
- **Indexes are declared, not copied.** A table that maps an email to a user's key is the application's own index. Leave it out of the copy and declare a unique index on `email`: the engine fills it as the objects go in.

## Declare the schema

The example moves a file of four tables, as an application would have defined them:

```rust
use redb::{MultimapTableDefinition, TableDefinition};

// name, email, age
const USERS: TableDefinition<u64, (&str, Option<&str>, u32)> = TableDefinition::new("users");
// author, body, published_at
const POSTS: TableDefinition<&str, (Option<u64>, &str, Option<&str>)> =
    TableDefinition::new("posts");
// The tags of each post, under its slug.
const POST_TAGS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("post_tags");
// The key of the user who has each email.
const USERS_BY_EMAIL: TableDefinition<&str, u64> = TableDefinition::new("users_by_email");
```

`users` keeps its integer key as the automatic `id`, the author becomes a link, the multimap table becomes a list of tags on each post, indexed so that a query by tag reads only the posts that have it, and `users_by_email` becomes a unique index:

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

The program opens the redb file read-only and writes a new DaruDB file:

```toml
[dependencies]
darudb = "1.3"
redb = "4.3"
```

```rust
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use redb::{ReadOnlyDatabase, ReadableDatabase, ReadableTable};

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
    let redb = ReadOnlyDatabase::open("app.redb")?;
    let rtxn = redb.begin_read()?;
    let users = rtxn.open_table(USERS)?;
    let posts = rtxn.open_table(POSTS)?;
    let post_tags = rtxn.open_multimap_table(POST_TAGS)?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    copy(
        &db,
        "users",
        users.iter()?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (id, user) = entry?;
            let (name, email, age) = user.value();

            Ok(Object::new()
                .with("id", i64::try_from(id.value())?)
                .with("name", name)
                .with("email", email)
                .with("age", age))
        }),
    )?;

    copy(
        &db,
        "posts",
        posts.iter()?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (slug, post) = entry?;
            let (author, body, published_at) = post.value();
            // The multimap gives a post's tags in the order of their bytes.
            let mut tags = Vec::new();

            for tag in post_tags.get(slug.value())? {
                tags.push(Value::from(tag?.value()));
            }

            Ok(Object::new()
                .with("slug", slug.value())
                .with("author", author.map(i64::try_from).transpose()?)
                .with("body", body)
                .with("published_at", published_at)
                .with("tags", tags))
        }),
    )?;

    // Every deferred commit is durable once this returns.
    db.close()?;

    Ok(())
}
```

`ReadOnlyDatabase` opens the file without writing to it, so it stays as it was; the application has to be closed, since a redb file is open in one process at a time. Every object goes in with the key it had, so `author` names the right user whichever collection is copied first. A value of a type of the application's is read with that type in the `TableDefinition`, as the application reads it, and copied field by field.

## Check the copy

Compare `users.len()?` on each table with `len` on each collection, and run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

What the application did with ranges and its own index tables becomes a query, which the engine runs through an index when one fits:

| What the application did | DaruDB query language |
| --- | --- |
| Got the key from `users_by_email`, then the user from `users` | `email == $0`, on `users` |
| Walked every user, testing the age in the program | `age BETWEEN 18 AND 30` |
| Walked every user, keeping the ten oldest | `SORT BY age DESC LIMIT 10` |
| Got the tags of a post from `post_tags` | The post's `tags` field |
| Walked `post_tags` for the posts with a tag | `tags CONTAINS "news"`, on `posts` |
| Read a post, then its author by key | `author.name == "Alice"`, on `posts`, for posts by the author's name |

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **No index upkeep.** Writing an object updates every index declared on its collection in the same transaction, so the code that kept `users_by_email` and `post_tags` in step goes away.
- **Objects, not tuples.** Objects go in and come out as an `Object`, or as a struct with `#[derive(Object)]`, read straight from the record. See [Collections and objects](../guide/objects.md).
- **Several processes** can use one file at the same time: one writer, any number of readers. See [Several processes](../guide/processes.md).
- **Transactions** keep their shape: a read sees one snapshot until it ends, and a write commits or aborts as a whole. See [Transactions](../guide/transactions.md).
- **Encryption** of the whole file comes with the engine; see [Encryption](../guide/encryption.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
