---
layout: home

title: DaruDB
titleTemplate: An embedded database in one local file
description: An embedded database that keeps an application's data in one local file, for Rust, Node.js, Dart and Python. One engine written in Rust, with encryption, crash safety and several processes on one file built in.

hero:
  name: DaruDB
  text: An embedded database for Rust, Node.js, Dart and Python
  tagline: Objects, indexes and queries in one local file, with no server to run. One engine, written in Rust, does the work for every language, and encryption, crash safety and several processes on one file are part of it.
  image:
    src: /logo.webp
    alt: The DaruDB logo
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Introduction
      link: /guide/introduction
    - theme: alt
      text: GitHub
      link: https://github.com/jooy2/darudb

features:
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><circle cx="4.5" cy="4.5" r="2"/><circle cx="19.5" cy="4.5" r="2"/><circle cx="4.5" cy="19.5" r="2"/><circle cx="19.5" cy="19.5" r="2"/><path d="m6 6 3.8 3.8M18 6l-3.8 3.8M6 18l3.8-3.8M18 18l-3.8-3.8"/></svg>
    title: One engine, four languages
    details: The engine is written once, in Rust, and Node.js, Dart and Python reach it through thin bindings. A file written from one language reads the same from the others, and every error has the same code in all of them.
    link: /guide/introduction
    linkText: How it is built
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="4.5" y="10.5" width="15" height="10" rx="2"/><path d="M8 10.5V7a4 4 0 0 1 8 0v3.5"/><path d="M12 14.5v2"/></svg>
    title: Encrypted page by page
    details: Every page is sealed with an authenticated cipher, under a key you give or one derived from a password with Argon2id. Without the key a file shows nothing but its header, and a page changed without it is refused.
    link: /guide/encryption
    linkText: Encrypt a database
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.8 5 5.5v6c0 4.4 3 7.9 7 9.2 4-1.3 7-4.8 7-9.2v-6Z"/><path d="m8.8 12.2 2.2 2.2 4.2-4.4"/></svg>
    title: Survives crashes and power cuts
    details: A committed page is never overwritten in place, and a commit takes effect by changing one byte. A process killed mid-write loses no commit, and a power cut leaves every commit that waited for the disk whole.
    link: /guide/transactions
    linkText: What a commit promises
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="2.5" y="3.5" width="8" height="5.5" rx="1.3"/><rect x="2.5" y="15" width="8" height="5.5" rx="1.3"/><path d="M15 3.5h3.5l3 3V19a1.5 1.5 0 0 1-1.5 1.5h-5A1.5 1.5 0 0 1 13.5 19V5A1.5 1.5 0 0 1 15 3.5Z"/><path d="M10.5 6.2h3M10.5 17.8h3"/></svg>
    title: Several processes, one file
    details: Processes share a file through the operating system's file locks alone, never shared memory. One writes at a time, readers do not wait for it, and a process that dies leaves nothing locked behind.
    link: /guide/processes
    linkText: How processes share it
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m15.5 15.5 5 5"/><path d="M7.5 8.5h6M7.5 11.5h4"/></svg>
    title: Queries in code or as text
    details: Filter, sort and page through objects with a builder in your language or with the query language, through indexes, links and embedded objects. Both forms become the same query inside the engine.
    link: /guide/query-language
    linkText: See the query language
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3 8.5 4.5L12 12 3.5 7.5Z"/><path d="m3.5 12 8.5 4.5 8.5-4.5"/><path d="m3.5 16.5 8.5 4.5 8.5-4.5"/></svg>
    title: Schemas that move forward
    details: Declare a schema version. Opening an older file adds the new collections, fields and indexes and runs your migration for each version in between, all in one transaction.
    link: /guide/migrations
    linkText: Migrate a schema
---

## One file, every language

The same query from the four packages. Each one wraps the same engine, so the file one of them writes is a file the others open, with the same objects and the same answers.

<div class="home-parity">
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="rust" />Rust</p>

```rust
let read = db.begin_read()?;
let query = Query::new().filter(Filter::ge("age", 18));
let adults = read.collection("users")?.query(&query)?;
```

</div>
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="node" />Node.js</p>

```ts
const adults = db.read((txn) => {
  const users = txn.collection('users');

  return users.find((q) => q.where('age', '>=', 18));
});
```

</div>
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="dart" />Dart</p>

```dart
final adults = db.read(
  (txn) => txn
      .collection(userSchema)
      .find((q) => q.where(q.age.atLeast(18))),
);
```

</div>
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="python" />Python</p>

```python
with db.read() as txn:
    adults = txn.collection(User).find(F.age >= 18)
```

</div>
</div>

<p class="home-note">And as text, the same in every language: <code>age &gt;= 18</code></p>

## Your data, as objects

A collection holds objects with typed fields, and a query finds some of them in an order. Pick a query to see which of these objects it returns, and how each language writes it.

<QueryDemo />

## What it looks like in use

Pick a language. The examples below are written for it, and so is every page of the guide and the API you open afterwards.

<LangTabs />

### Declare a schema and write objects

::: lang rust

```rust
use darudb::{Collection, Object, OpenOptions, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1).collection(
        Collection::new("users")
            .field("name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::Int, 0)
            .unique("email")
            .index("age"),
    );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;
    users.insert(Object::new().with("name", "Ada").with("age", 36))?;
    users.insert(Object::new().with("name", "Ben").with("age", 24))?;
    txn.commit()?;

    db.close()
}
```

:::

::: lang node

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index()
  })
});

const db = Database.open('app.darudb', { schema: app });

db.write((txn) => {
  const users = txn.collection('users');

  users.insert({ name: 'Ada', age: 36 });
  users.insert({ name: 'Ben', age: 24 });
});
```

:::

::: lang dart

```dart
import 'package:darudb/darudb.dart';

part 'main.g.dart';

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

void main() {
  final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

  db.write((txn) {
    final users = txn.collection(userSchema);

    users.insert(const User(name: 'Ada', age: 36));
    users.insert(const User(name: 'Ben', age: 24));
  });
}
```

:::

::: lang python

```python
import darudb
from darudb import field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))

with db.write() as txn:
    users = txn.collection(User)

    users.insert(User(name="Ada", age=36))
    users.insert(User(name="Ben", age=24))
```

:::

### Find objects in code or as text

::: lang rust

```rust
use darudb::{Database, Filter, Query};

fn find(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    let built = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);
    let written = Query::parse(
        r#"age >= $0 AND name STARTSWITH "A" SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;

    assert_eq!(users.query(&built)?, users.query(&written)?);
    println!("{} adults", users.count(&Query::new().filter(Filter::ge("age", 18)))?);
    Ok(())
}
```

:::

::: lang node

```ts
db.read((txn) => {
  const users = txn.collection('users');

  const built = users.find((q) =>
    q.where('age', '>=', 18).where('name', 'startsWith', 'A').sortBy('age', 'desc').limit(10)
  );
  const written = users.find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [
    18,
    'A'
  ]);

  console.log(
    built,
    written,
    users.count((q) => q.where('age', '>=', 18))
  );
});
```

:::

::: lang dart

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  final built = users.find(
    (q) => q
        .where(q.age.atLeast(18) & q.name.startsWith('A'))
        .sortBy(q.age, descending: true)
        .limit(10),
  );
  final written = users.findText(
    r'age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10',
    [18, 'A'],
  );

  print('$built $written ${users.count((q) => q.where(q.age.atLeast(18)))}');
});
```

:::

::: lang python

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    built = users.find(
        where((F.age >= 18) & F.name.startswith("A")).sort_by(F.age, descending=True).limit(10)
    )
    written = users.find("age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10", 18, "A")

    print(built, written, users.count(F.age >= 18))
```

:::

### Encrypt the file and keep it healthy

::: lang rust

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    assert!(db.check()?.is_ok());
    db.backup("backups/secret.darudb")?;
    db.close()
}
```

:::

::: lang node

```ts
const db = await Database.openAsync('secret.darudb', {
  schema: app,
  password: 'correct horse battery staple'
});

if ((await db.checkAsync()).ok) {
  await db.backupAsync('backups/secret.darudb');
}
await db.closeAsync();
```

:::

::: lang dart

```dart
final db = await Database.openAsync(
  'secret.darudb',
  schema: const Schema(1, [userSchema]),
  password: 'correct horse battery staple',
);

if ((await db.checkAsync()).ok) {
  await db.backupAsync('backups/secret.darudb');
}
await db.closeAsync();
```

:::

::: lang python

```python
db = await darudb.Database.open_async(
    "secret.darudb",
    schema=darudb.Schema(1, [User]),
    password="correct horse battery staple",
)

if (await db.check_async()).ok:
    await db.backup_async("backups/secret.darudb")
await db.close_async()
```

:::

## What is inside

Every package has all of it, except that the Rust crate has no asynchronous API of its own. Each card opens the guide that covers it.

<GuideGrid :items="[
  { title: 'Collections and objects', desc: 'Typed fields, primary keys or ids the engine assigns, unique and secondary indexes, links, lists and embedded objects.', link: '/guide/objects' },
  { title: 'Queries', desc: 'Comparisons, ranges, IN, string matching and null tests, sorted and paged, built in code or written as text.', link: '/guide/queries' },
  { title: 'Query language', desc: 'Every operator, keyword and value of the text form, with an example of each.', link: '/guide/query-language' },
  { title: 'Transactions', desc: 'Snapshots for reading, and commits that wait for the disk or leave the sync for later.', link: '/guide/transactions' },
  { title: 'Migrations', desc: 'Schema versions, renames, and a function for each version step, in one transaction.', link: '/guide/migrations' },
  { title: 'Async APIs', desc: 'A Promise, Future or asyncio twin of every call that uses the file, in Node.js, Dart and Python.', link: '/guide/async' },
  { title: 'Encryption', desc: 'Authenticated encryption of every page, keys from a password, and changing either in place.', link: '/guide/encryption' },
  { title: 'Several processes', desc: 'One writer and any number of readers across processes, through file locks alone.', link: '/guide/processes' },
  { title: 'Tools', desc: 'An integrity check, online backup, compaction, and salvage of a damaged file.', link: '/guide/tools' },
  { title: 'Errors', desc: 'One stable code for every failure, the same in every language.', link: '/guide/errors' }
]" />

## Start with your language

Add the package for your language. The getting-started page opens in it, with its commands and its code.

<StartCards :cards="[
  { id: 'rust', note: 'Rust 1.85 or later. The derive feature reads and writes your structs directly.', install: 'cargo add darudb --features derive', link: '/guide/getting-started' },
  { id: 'node', note: 'Node.js 20 or later, typed for TypeScript, with the engine prebuilt for every platform it supports.', install: 'npm install darudb', link: '/guide/getting-started' },
  { id: 'dart', note: 'Dart 3.10 or Flutter 3.38.1 or later. The build hook fetches the engine prebuilt for each target.', install: 'dart pub add darudb dev:darudb_generator dev:build_runner', link: '/guide/getting-started' },
  { id: 'python', note: 'CPython 3.11 or later, free-threaded 3.14 included, with wheels for Linux, macOS and Windows.', install: 'pip install darudb', link: '/guide/getting-started' }
]" />

<div class="home-cta">

[Introduction](/guide/introduction) [Getting started](/guide/getting-started) [API](/api/) [Comparison](/compare)

</div>
