---
title: SQLite
order: 2
---

# Migrating from SQLite

This page moves the data of a SQLite database into DaruDB: how tables, columns and indexes map onto collections and fields, a script that copies every row, and how the application's queries change.

## How the two map

| SQLite | DaruDB |
| --- | --- |
| A table | A collection |
| A row | An object |
| `INTEGER PRIMARY KEY`, the rowid | The automatic `id`, or a declared integer key |
| `PRIMARY KEY` on another column | A primary key on that field, of type integer, string or bytes |
| A primary key on several columns | No counterpart: use the automatic `id`, or one string key built from the parts |
| `INTEGER` | An integer field, 64-bit |
| `REAL` | A float field, 64-bit |
| `TEXT` | A string field |
| `BLOB` | A bytes field |
| `NOT NULL` | A required field; a column that allows `NULL` is an optional field |
| `DEFAULT` | A default on the field |
| `UNIQUE`, or a unique index on one column | A unique index |
| An index on one column | An index |
| An index on several columns | No counterpart yet: index the column that narrows a query most |
| A foreign key | A link, which holds the other object's primary key |
| A join table between two tables | A list of links on one side, or a list of values |
| A view, a trigger, a `CHECK` constraint | No counterpart: keep that logic in the application |

- **Types are checked.** A SQLite column can hold a value of any type, whatever type it declares; a DaruDB field holds values of its own type only. Look for the strays before copying, such as `SELECT count(*) FROM users WHERE typeof(age) != 'integer'`, and convert them in the script, or the copy stops with `INVALID_ARGUMENT` at the first one.
- **Booleans** stored as `0` and `1` can become a boolean field, with the script converting each value.
- **Dates and times** have no type of their own in either database. Text in ISO 8601, in UTC, sorts in time order as bytes, so it can stay a string field and still be compared and sorted; a number of seconds or milliseconds stays an integer.
- **Keys carry over.** Copying each row's key as the object's key keeps every foreign key valid as a link, and a collection with the automatic `id` goes on numbering after the largest `id` copied into it.

## Declare the schema

The example moves this database: users, their posts, and the tags of each post in a join table.

```sql
CREATE TABLE users (
  id    INTEGER PRIMARY KEY,
  name  TEXT NOT NULL,
  email TEXT UNIQUE,
  age   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX users_age ON users (age);

CREATE TABLE posts (
  slug         TEXT PRIMARY KEY,
  author_id    INTEGER REFERENCES users (id),
  body         TEXT NOT NULL,
  published_at TEXT
);
CREATE INDEX posts_author ON posts (author_id);

CREATE TABLE post_tags (
  post_slug TEXT NOT NULL REFERENCES posts (slug),
  tag       TEXT NOT NULL,
  PRIMARY KEY (post_slug, tag)
);
```

`users` keeps its integer key as the automatic `id`, `author_id` becomes a link, and the join table becomes a list of tags on each post, indexed so that a query by tag reads only the posts that have it.

::: lang rust

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

::: lang dart

```dart
import 'package:darudb/darudb.dart';

part 'schema.g.dart';

@Collection('users')
class User {
  const User({this.id, required this.name, this.email, this.age = 0});

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
}

@Collection('posts')
class Post {
  const Post({
    required this.slug,
    this.author,
    required this.body,
    this.publishedAt,
    required this.tags,
  });

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
  final String body;
  @Name('published_at')
  final String? publishedAt;
  @Index()
  final List<String> tags;
}

const app = Schema(1, [userSchema, postSchema]);
```

:::

## Copy the rows

::: lang rust

The script reads the old file with the `rusqlite` crate, whose `bundled` feature compiles SQLite into the program:

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb" }
rusqlite = { version = "0.40", features = ["bundled"] }
```

```rust
use std::collections::HashMap;
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use rusqlite::{Connection, OpenFlags};

/// Inserts `objects` into collection `name`, in one deferred commit.
fn insert_all(db: &Database, name: &str, objects: Vec<Object>) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    let mut collection = txn.collection(name)?;

    for object in objects {
        collection.insert(object)?;
    }

    txn.commit_deferred()
}

fn main() -> Result<(), Box<dyn Error>> {
    let sqlite = Connection::open_with_flags("app.sqlite", OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    let mut users = sqlite.prepare("SELECT id, name, email, age FROM users")?;
    let mut rows = users.query([])?;
    let mut batch = Vec::new();

    while let Some(row) = rows.next()? {
        batch.push(
            Object::new()
                .with("id", row.get::<_, i64>(0)?)
                .with("name", row.get::<_, String>(1)?)
                .with("email", row.get::<_, Option<String>>(2)?)
                .with("age", row.get::<_, i64>(3)?),
        );

        if batch.len() == 1000 {
            insert_all(&db, "users", std::mem::take(&mut batch))?;
        }
    }

    insert_all(&db, "users", batch)?;

    // The join table becomes a list on each post.
    let mut tags: HashMap<String, Vec<Value>> = HashMap::new();
    let mut post_tags = sqlite.prepare("SELECT post_slug, tag FROM post_tags")?;
    let mut rows = post_tags.query([])?;

    while let Some(row) = rows.next()? {
        tags.entry(row.get(0)?).or_default().push(Value::from(row.get::<_, String>(1)?));
    }

    let mut posts = sqlite.prepare("SELECT slug, author_id, body, published_at FROM posts")?;
    let mut rows = posts.query([])?;
    let mut batch = Vec::new();

    while let Some(row) = rows.next()? {
        let slug: String = row.get(0)?;

        batch.push(
            Object::new()
                .with("tags", tags.remove(&slug).unwrap_or_default())
                .with("slug", slug)
                .with("author", row.get::<_, Option<i64>>(1)?)
                .with("body", row.get::<_, String>(2)?)
                .with("published_at", row.get::<_, Option<String>>(3)?),
        );

        if batch.len() == 1000 {
            insert_all(&db, "posts", std::mem::take(&mut batch))?;
        }
    }

    insert_all(&db, "posts", batch)?;

    // Every deferred commit is durable once this returns.
    db.close()?;

    Ok(())
}
```

:::

::: lang node

The script reads the old file with `node:sqlite`, which Node.js has built in from 22.13 on, so there is nothing to install for it:

```js
import { DatabaseSync } from 'node:sqlite';
import { Database } from 'darudb';

const sqlite = new DatabaseSync('app.sqlite', { readOnly: true });
const db = Database.open('app.darudb', { schema: app });

/** Copies `rows` into collection `name`, a thousand to a transaction. */
function copy(rows, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for (const row of rows) {
    batch.push(convert(row));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

copy(sqlite.prepare('SELECT id, name, email, age FROM users').iterate(), 'users', (row) => ({
  id: row.id,
  name: row.name,
  email: row.email,
  age: row.age
}));

// The join table becomes a list on each post.
const tags = new Map();

for (const { post_slug, tag } of sqlite.prepare('SELECT post_slug, tag FROM post_tags').iterate()) {
  tags.set(post_slug, [...(tags.get(post_slug) ?? []), tag]);
}

copy(
  sqlite.prepare('SELECT slug, author_id, body, published_at FROM posts').iterate(),
  'posts',
  (row) => ({
    slug: row.slug,
    author: row.author_id,
    body: row.body,
    publishedAt: row.published_at,
    tags: tags.get(row.slug) ?? []
  })
);

// Every deferred commit is durable once this returns.
db.close();
sqlite.close();
```

`node:sqlite` reads an integer as a number. For a column whose values go beyond 2^53, call `setReadBigInts(true)` on the statement, and declare the field with `t.bigint()`.

:::

::: lang dart

The script reads the old file with the `sqlite3` package, whose build hook brings SQLite with it:

```yaml
dependencies:
  darudb:
    path: ../darudb/packages/dart/darudb
  sqlite3: ^3.7.0
```

```dart
import 'package:darudb/darudb.dart';
import 'package:sqlite3/sqlite3.dart';

import 'schema.dart';

void main() {
  final sqlite = sqlite3.open('app.sqlite', mode: OpenMode.readOnly);
  final db = Database.open('app.darudb', schema: app);

  /// Copies [rows] into a collection, a thousand to a transaction.
  void copy<T>(
    Iterable<Row> rows,
    CollectionSchema<T, QueryBuilder<T>, Object> collection,
    T Function(Row row) convert,
  ) {
    final batch = <T>[];

    void flush() {
      db.write(
        (txn) => txn.collection(collection).insertMany(batch),
        durability: Durability.deferred,
      );
      batch.clear();
    }

    for (final row in rows) {
      batch.add(convert(row));

      if (batch.length == 1000) {
        flush();
      }
    }

    if (batch.isNotEmpty) {
      flush();
    }
  }

  copy(
    sqlite.select('SELECT id, name, email, age FROM users'),
    userSchema,
    (row) => User(
      id: row['id'] as int,
      name: row['name'] as String,
      email: row['email'] as String?,
      age: row['age'] as int,
    ),
  );

  // The join table becomes a list on each post.
  final tags = <String, List<String>>{};

  for (final row in sqlite.select('SELECT post_slug, tag FROM post_tags')) {
    tags.putIfAbsent(row['post_slug'] as String, () => []).add(row['tag'] as String);
  }

  copy(
    sqlite.select('SELECT slug, author_id, body, published_at FROM posts'),
    postSchema,
    (row) => Post(
      slug: row['slug'] as String,
      author: row['author_id'] == null ? null : Link<User>(row['author_id'] as int),
      body: row['body'] as String,
      publishedAt: row['published_at'] as String?,
      tags: tags[row['slug']] ?? [],
    ),
  );

  // Every deferred commit is durable once this returns.
  db.close();
  sqlite.close();
}
```

:::

The old file is opened read-only and stays as it was. Copying the whole join table into memory first is what lets each post be written once, with its tags; for a join table too large for memory, read it ordered by `post_slug` beside the posts ordered by `slug` instead.

## Check the copy

Count each table and each collection, and compare: `SELECT count(*) FROM users` against <LangCode rust="len" node="count" dart="count" /> on `users`. Then run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

The query language reads much like a `WHERE` clause:

| SQL | DaruDB query language |
| --- | --- |
| `WHERE age >= 18` | `age >= 18` |
| `WHERE email IS NULL` | `email IS NULL` |
| `WHERE age BETWEEN 18 AND 30` | `age BETWEEN 18 AND 30` |
| `WHERE id IN (1, 2)` | `id IN [1, 2]` |
| `WHERE name LIKE 'A%'` | `name STARTSWITH "A"` |
| `WHERE name LIKE '%li%'` | `name CONTAINS "li"` |
| `ORDER BY age DESC LIMIT 10 OFFSET 20` | `SORT BY age DESC LIMIT 10 OFFSET 20` |
| `SELECT count(*) ...` | `count`, with the same query |
| `JOIN users ON users.id = posts.author_id WHERE users.name = 'Alice'` | `author.name == "Alice"`, on `posts` |
| `JOIN post_tags ... WHERE tag = 'news'` | `tags CONTAINS "news"`, on `posts` |

- `LIKE` ignores the case of ASCII letters, and `STARTSWITH` and `CONTAINS` compare bytes, so they do not. A case-insensitive search keeps a lowercased copy of the field and queries that.
- A join becomes a path through a link, which reads the linked object. There is no counterpart for `GROUP BY`, for aggregates other than a count, for subqueries, or for a join on anything but a link.
- Values from outside the program go in parameters, `$0` and on, as they would in a prepared statement. [Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **Transactions** are <LangCode rust="begin_read and begin_write" node="db.read and db.write" dart="db.read and db.write" /> instead of `BEGIN` and `COMMIT`, and a read happens in one too. [Transactions](../guide/transactions.md) says what a commit promises.
- **Schema changes** raise the schema's version instead of running `ALTER TABLE`: the engine adds new collections, fields and indexes by itself, and a migration names the rest. See [Migrations](../guide/migrations.md).
- **Results are plain objects**, which keep their values after the transaction ends.
