---
title: LevelDB
order: 7
languages: [node]
---

# Migrating from LevelDB

This page moves the data of a LevelDB database, written through `classic-level` or `level`, into DaruDB: how sublevels, keys and values map onto collections, objects and fields, a script that copies every entry, and what changes in the application.

## How the two map

| LevelDB through `classic-level` | DaruDB |
| --- | --- |
| A database: a folder of files | A database: one file |
| A sublevel whose values are records | A collection |
| A sublevel the application keeps as an index, from a field's value to a key | A declared index or unique index, which the engine keeps in step with the objects |
| A key, a string | The primary key, here a string; or an integer, when the keys are numbers |
| A value in `json` encoding | An object, with a field for each of its properties |
| `batch` | A write transaction, which commits or aborts as a whole |
| A snapshot that an iterator reads from | A read transaction |
| `sync: true` on a write | A sync commit, the default |
| A write without `sync` | A deferred commit |
| An `on('put')` listener, a hook | No counterpart: read again after a write |

- **The schema comes from the application.** LevelDB stores bytes, and the `json` encoding writes any value, so the schema is declared from the objects the application writes into each sublevel. The engine checks every value against its field's type, and the copy stops with `INVALID_ARGUMENT` at the first that does not fit.
- **Keys carry over.** A string key stays a string key, so every reference to it stays valid as a link. Keys the application built from parts, such as `post!2026-10!42`, become a field of each part and an index.
- **Indexes are declared, not copied.** A sublevel that maps an email to a user's key is the application's own index. Leave it out of the copy and declare a unique index on `email`: the engine fills it as the objects go in.
- **Operations are synchronous.** `classic-level` answers through promises; DaruDB's `read` and `write` return their result, and an asynchronous API with the same shapes runs on the thread pool. See [Asynchronous API](../guide/async.md).

## Declare the schema

The example moves a database of three sublevels, as an application would have written them, with `json` values:

| Sublevel | Key | Value |
| --- | --- | --- |
| `users` | The user's id, a string | `name`, `email`, which may be `null`, and `age` |
| `posts` | The post's slug | `authorId`, the id of a user, `body`, `publishedAt` and a list of `tags` |
| `users_by_email` | An email | The id of the user who has it |

`users` keeps its string key as a declared primary key, `authorId` becomes a link, and `users_by_email` becomes a unique index:

```ts
import { collection, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    id: t.string().primaryKey(),
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

## Copy the entries

The script reads the database with `classic-level`, which `level` uses in Node.js:

```js
import { ClassicLevel } from 'classic-level';
import { Database } from 'darudb';

const level = new ClassicLevel('app.level', { createIfMissing: false });
const users = level.sublevel('users', { valueEncoding: 'json' });
const posts = level.sublevel('posts', { valueEncoding: 'json' });
const db = Database.open('app.darudb', { schema: app });

/** Copies `entries` into collection `name`, a thousand to a transaction. */
async function copy(entries, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for await (const entry of entries) {
    batch.push(convert(entry));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

await copy(users.iterator(), 'users', ([key, value]) => ({
  id: key,
  name: value.name,
  email: value.email,
  age: value.age
}));

await copy(posts.iterator(), 'posts', ([key, value]) => ({
  slug: key,
  author: value.authorId,
  body: value.body,
  publishedAt: value.publishedAt,
  tags: value.tags
}));

// Every deferred commit is durable once this returns.
db.close();
await level.close();
```

- **The application has to be closed.** A LevelDB database is open in one process at a time, and it has no read-only mode: copy from a copy of the folder to be sure nothing changes the original.
- **Each iterator reads one snapshot**, taken when it is made, so writes made meanwhile do not reach the copy.
- **Sublevels are named as the application named them**, with the same encodings. One opened with `keyEncoding` or `valueEncoding` other than the defaults is opened with those here.

## Check the copy

Count the entries of each sublevel, `(await users.keys().all()).length`, and compare them with `count` on each collection. Then run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

What the application did with iterators and its own index sublevels becomes a query, which the engine runs through an index when one fits:

| What the application did | DaruDB query language |
| --- | --- |
| Got the id from `users_by_email`, then the user from `users` | `email == $0`, on `users` |
| Iterated `users` with `gte` and `lt` on a key prefix | `STARTSWITH` on a field of that part of the key |
| Iterated `users`, testing the age in the program | `age BETWEEN 18 AND 30` |
| Iterated with `reverse: true` and `limit: 10` | `SORT BY age DESC LIMIT 10` |
| Iterated `posts` for the ones with a tag | `tags CONTAINS "news"`, on `posts` |
| Read a post, then its author by id | `author.name == "Alice"`, on `posts`, for posts by the author's name |

[Queries](../guide/queries.md) has the whole language, and the builder, whose types TypeScript infers from the schema.

## What changes in the application

- **No index upkeep.** Writing an object updates every index declared on its collection in the same transaction, so the code that kept `users_by_email` in step goes away.
- **Objects of the schema's types.** TypeScript infers every object's type from the schema, and the engine checks each field when it is written.
- **Several processes** can use one file at the same time, such as an Electron app's main process and a helper: one writer, any number of readers. See [Several processes](../guide/processes.md).
- **Transactions** are `db.read` and `db.write`, scoped to a function, which commits when it returns and aborts when it throws. See [Transactions](../guide/transactions.md).
- **Encryption** of the whole file comes with the engine; see [Encryption](../guide/encryption.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
