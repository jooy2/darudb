---
title: LMDB
order: 4
languages: [rust, node, python]
---

# Migrating from LMDB

This page moves the data of an LMDB environment into DaruDB: how named databases, keys and values map onto collections, objects and fields, a program that copies every entry, through heed in Rust, lmdb-js in Node.js or the `lmdb` module in Python, and what changes in the application.

## How the two map

| LMDB | DaruDB |
| --- | --- |
| An environment: a folder with `data.mdb` and `lock.mdb` | A database: one file |
| A named database whose values are records | A collection |
| A named database the application keeps as an index, from a field's value to a key | A declared index or unique index, which the engine keeps in step with the objects |
| A key | The primary key, of type integer, string or bytes |
| A value | An object, decoded from its bytes the way the application wrote them |
| A `dupsort` database, several values under one key | A list field on the object, or a collection of its own |
| A read transaction, a write transaction | A read transaction, a write transaction |
| A commit with `NOSYNC` or `NO_SYNC` | A deferred commit |
| The map size | Nothing to set: the file grows as it needs |

- **The schema comes from the application.** LMDB stores bytes and knows nothing of their shape, so the schema is declared from what the application writes into each named database. The engine checks every value against its field's type, and the copy stops with `INVALID_ARGUMENT` at the first that does not fit: decode each value as the application does, and convert the strays in the program.
- **Keys carry over.** An integer key stored big-endian, so that it sorts as a number, becomes an integer key; a string key stays a string; any other key can stay bytes. A key the application built from parts, such as `user/42/post/7`, becomes a field of each part and an index, or a string key.
- **Indexes are declared, not copied.** A named database that maps an email to a user's key is the application's own index. Leave it out of the copy and declare a unique index on `email`: the engine fills it as the objects go in, and keeps it right from then on.

## Declare the schema

The example moves an environment of three named databases, as an application would have written them:

| Named database | Key | Value |
| --- | --- | --- |
| `users` | The user's id, an unsigned 64-bit integer, big-endian | `name`, `email`, which may be missing, and `age` |
| `posts` | The post's slug, a string | `author_id`, the key of a user, `body`, `published_at` and a list of `tags`, named `authorId` and `publishedAt` from Node.js |
| `users_by_email` | An email | The key of the user who has it |

::: lang rust

The values are JSON, written with heed's `SerdeJson`. In DaruDB, `users` keeps its integer key as the automatic `id`, `author_id` becomes a link, and `users_by_email` becomes a unique index:

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

:::

::: lang node

The values are lmdb-js's default encoding, MessagePack, and the keys its default ordered encoding, in which a number sorts as a number. In DaruDB, `users` keeps its integer key as the automatic `id`, `authorId` becomes a link, and `users_by_email` becomes a unique index:

```ts
import { collection, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index()
  }),
  posts: collection({
    slug: t.string().primaryKey(),
    author: t.link('users').optional().index(),
    body: t.string(),
    publishedAt: t.string().optional(),
    tags: t.list(t.string()).index()
  })
});
```

:::

::: lang python

The values are JSON, and the keys of `users` are packed with `struct.pack(">Q", id)`. In DaruDB, `users` keeps its integer key as the automatic `id`, `author_id` becomes a link, and `users_by_email` becomes a unique index:

```python
import darudb
from darudb import field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


@darudb.collection("posts")
class Post:
    slug: str = field(primary_key=True)
    author: int | None = field(default=None, link=User, index=True)
    body: str
    published_at: str | None = None
    tags: list[str] = field(index=True)


app = darudb.Schema(1, [User, Post])
```

:::

## Copy the entries

::: lang rust

The program reads the environment with heed, the crate the application wrote it with, and decodes each value into a struct with serde:

```toml
[dependencies]
darudb = "1.3"
heed = "0.22"
serde = { version = "1", features = ["derive"] }
```

```rust
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use heed::byteorder::BigEndian;
use heed::types::{SerdeJson, Str, U64};
use heed::{EnvFlags, EnvOpenOptions};
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
    // SAFETY: LMDB maps the file, and an environment opened twice in one
    // process breaks its locks; this program opens it once, read-only.
    let env = unsafe {
        EnvOpenOptions::new()
            .max_dbs(8)
            .flags(EnvFlags::READ_ONLY)
            .open("app.lmdb")?
    };
    let rtxn = env.read_txn()?;
    let users: heed::Database<U64<BigEndian>, SerdeJson<User>> =
        env.open_database(&rtxn, Some("users"))?.ok_or("no users database")?;
    let posts: heed::Database<Str, SerdeJson<Post>> =
        env.open_database(&rtxn, Some("posts"))?.ok_or("no posts database")?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    copy(
        &db,
        "users",
        users.iter(&rtxn)?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (id, user) = entry?;

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
        posts.iter(&rtxn)?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (slug, post) = entry?;
            let author = post.author_id.map(i64::try_from).transpose()?;

            Ok(Object::new()
                .with("slug", slug)
                .with("author", author)
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

An integer key of DaruDB is signed, so a `u64` above `i64::MAX` cannot be one: `try_from` stops the copy there, and such keys can stay bytes instead, `Type::Bytes` with `id.to_be_bytes()`. Values written with another serde format are read the same way, with heed's `SerdeBincode` or `SerdeRmp` in place of `SerdeJson`.

:::

::: lang node

The script reads the environment with lmdb-js, the package the application wrote it with:

```js
import { open } from 'lmdb';
import { Database } from 'darudb';

const lmdb = open({ path: 'app.lmdb', readOnly: true });
const users = lmdb.openDB('users');
const posts = lmdb.openDB('posts');
const db = Database.open('app.darudb', { schema: app });

/** Copies `entries` into collection `name`, a thousand to a transaction. */
function copy(entries, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for (const entry of entries) {
    batch.push(convert(entry));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

copy(users.getRange(), 'users', ({ key, value }) => ({
  id: key,
  name: value.name,
  email: value.email ?? null,
  age: value.age
}));

copy(posts.getRange(), 'posts', ({ key, value }) => ({
  slug: key,
  author: value.authorId ?? null,
  body: value.body,
  publishedAt: value.publishedAt ?? null,
  tags: value.tags
}));

// Every deferred commit is durable once this returns.
db.close();
await lmdb.close();
```

`openDB` takes the same `encoding` and `keyEncoding` the application opened each database with, such as `encoding: 'json'`, and `getRange` walks the database in key order from one read snapshot.

:::

::: lang python

The script reads the environment with the `lmdb` module, the package the application wrote it with:

```python
import json
import struct

import darudb
import lmdb

from schema import Post, User, app

env = lmdb.open("app.lmdb", readonly=True, max_dbs=8)
users = env.open_db(b"users", create=False)
posts = env.open_db(b"posts", create=False)
db = darudb.Database.open("app.darudb", schema=app)


def copy(entries, cls, convert):
    """Copies ``entries`` into the collection of ``cls``, a thousand to a transaction."""
    batch = []

    def flush():
        with db.write(durability="deferred") as txn:
            txn.collection(cls).insert_many(batch)

        batch.clear()

    for entry in entries:
        batch.append(convert(entry))

        if len(batch) == 1000:
            flush()

    if batch:
        flush()


def user(key, value):
    fields = json.loads(value)

    return User(id=struct.unpack(">Q", key)[0], name=fields["name"], email=fields["email"], age=fields["age"])


def post(key, value):
    fields = json.loads(value)

    return Post(
        slug=key.decode(),
        author=fields["author_id"],
        body=fields["body"],
        published_at=fields["published_at"],
        tags=fields["tags"],
    )


with env.begin() as txn:
    copy(txn.cursor(db=users), User, lambda entry: user(*entry))
    copy(txn.cursor(db=posts), Post, lambda entry: post(*entry))

# Every deferred commit is durable once this returns.
db.close()
env.close()
```

A cursor walks a named database in key order from the transaction's snapshot, giving each key and value as `bytes`. Values the application pickled or packed with MessagePack are read with `pickle.loads` or `msgpack.unpackb` in place of `json.loads`.

:::

The environment is opened read-only and stays as it was. Every object goes in with the key it had, so `author` names the right user whichever collection is copied first.

## Check the copy

Count each named database and each collection, and compare: <LangCode rust="users.len(&rtxn)" node="users.getCount()" python="txn.stat(users)['entries']" /> against <LangCode rust="len" node="count" python="count" /> on `users`. Then run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

What the application did with cursors and its own index databases becomes a query, which the engine runs through an index when one fits:

| What the application did | DaruDB query language |
| --- | --- |
| Looked the email up in `users_by_email`, then the key in `users` | `email == $0`, on `users` |
| Walked an index database of ages from 18 to 30 | `age BETWEEN 18 AND 30` |
| Walked every user, testing a field in the program | The test as a query: `name STARTSWITH "A"` |
| Walked an index database of ages backwards, keeping ten | `SORT BY age DESC LIMIT 10` |
| Kept a `dupsort` database of tags to posts | `tags CONTAINS "news"`, on `posts` |
| Read a post, then its author by key | `author.name == "Alice"`, on `posts`, for posts by the author's name |

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **No index upkeep.** Writing an object updates every index declared on its collection in the same transaction, so the code that kept `users_by_email` in step goes away.
- **No encoding.** Objects go in and come out as <LangCode rust="an Object, or a struct with #[derive(Object)]" node="plain objects of the schema's types" python="instances of the collection's class" />, so the code that encoded and decoded values goes away too.
- **Transactions** keep their shape: one writer at a time, any number of readers, across threads and processes. A read sees one snapshot until it ends. See [Transactions](../guide/transactions.md).
- **No map size.** The file grows as it needs, and [compacting](../guide/tools.md#compact-a-file) it gives free space back.
- **Encryption** of the whole file comes with the engine; see [Encryption](../guide/encryption.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
