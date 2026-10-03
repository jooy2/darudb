---
title: Getting started
order: 2
---

# Getting started

DaruDB is not published yet, so for now you build it from source, add it to your project by path, and open a first database.

## Requirements

::: lang rust

- **Rust**, installed with [rustup](https://rustup.rs). The repository pins its compiler in `rust-toolchain.toml`, and `rustup` installs that version on the first build. A program that depends on the `darudb` crate needs Rust 1.85 or later.

:::

::: lang node

- **Node.js 20 or later.** Building the package from source needs a little more, because of its build tool: 20.17 or later on the 20 line, or 22.13 or later.
- **Rust**, installed with [rustup](https://rustup.rs), since the package's native addon is compiled from the engine. The repository pins the compiler in `rust-toolchain.toml`, and `rustup` installs it on the first build.

:::

::: lang dart

- **Dart 3.10 or later**, or Flutter 3.38 or later: the package builds its native library in a build hook, which those releases made stable.
- **Rust**, installed with [rustup](https://rustup.rs). Until the package is published with prebuilt libraries, its build hook compiles the engine, with the compiler `packages/dart/darudb/native/rust-toolchain.toml` pins, which `rustup` installs on the first build with the targets it lists.

:::

- **Git**, to clone the repository.

DaruDB runs on Unix-like systems and on Windows. Network file systems such as NFS and SMB are not supported, because their file locks and syncs do not keep the promises a database relies on.

## Build from source

```bash
git clone https://github.com/jooy2/darudb.git
cd darudb
```

::: lang rust

```bash
cargo test -p darudb
```

`cargo test` builds the engine and runs its tests, which is the quickest way to know your toolchain works.

:::

::: lang node

```bash
cd packages/node
npm install
npm run build
```

`npm run build` compiles the engine and the binding into one addon for your platform, writes the files that load it, and compiles the TypeScript API into `dist/`. `npm test` then runs the package's tests against that build.

:::

::: lang dart

```bash
cd packages/dart/darudb
dart pub get
dart run build_runner build
dart test
```

`dart test` runs the package's build hook first, which compiles the engine for your platform, so the first run takes a minute or two. `build_runner` writes the code of the tests' models, as it does for an application's.

:::

## Add it to a project

::: lang rust

Add the crate by path until it is published:

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb" }
```

The `derive` feature adds `#[derive(Object)]`, which makes a struct the objects of a collection; see [Collections and objects](./objects.md#objects-as-rust-types).

:::

::: lang node

Install the folder you built, which links it into your project:

```bash
npm install ../darudb/packages/node
```

The package is TypeScript-first: its declarations ship with it, and the types of your objects follow from the schema you declare. It works from JavaScript as well.

:::

::: lang dart

Add the package by path until it is published, with the generator that writes the code for your classes:

```yaml
dependencies:
  darudb:
    path: ../darudb/packages/dart/darudb

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator:
    path: ../darudb/packages/dart/darudb_generator
```

The package works in Flutter apps, Dart servers and command-line tools alike. [Collections and objects](./objects.md) shows the classes the generator reads.

:::

## Open a database

A database is one file. Opening a path where nothing exists creates the file; opening an existing file checks that it is a DaruDB database this build can read.

::: lang rust

```rust
use darudb::{Database, OpenOptions};

fn main() -> Result<(), darudb::Error> {
    let db = Database::open("app.darudb")?;
    println!("page size: {} bytes", db.page_size());
    db.close()?;

    // Open only if the file is already there.
    let db = OpenOptions::new().create(false).open("app.darudb")?;
    db.close()
}
```

:::

::: lang node

```ts
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();

// Open only if the file is already there.
Database.open('app.darudb', { create: false }).close();
```

:::

::: lang dart

```dart
import 'package:darudb/darudb.dart';

void main() {
  final db = Database.open('app.darudb');

  print('page size: ${db.pageSize} bytes');
  db.close();

  // Open only if the file is already there.
  Database.open('app.darudb', create: false).close();
}
```

:::

## Store your first objects

A schema names the collections the database holds and the fields of their objects. Open the database with it, write objects in a write transaction, and find them again in a read transaction.

::: lang rust

```rust
use darudb::{Collection, Filter, Object, OpenOptions, Query, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1).collection(
        Collection::new("users")
            .field("name", Type::String)
            .with_default("age", Type::Int, 0)
            .index("age"),
    );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;
    users.insert(Object::new().with("name", "Alice").with("age", 31))?;
    users.insert(Object::new().with("name", "Bob").with("age", 17))?;
    txn.commit()?;

    let read = db.begin_read()?;
    let adults = read
        .collection("users")?
        .query(&Query::new().filter(Filter::ge("age", 18)))?;
    println!("{adults:?}");

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
    age: t.int().default(0).index()
  })
});

const db = Database.open('app.darudb', { schema: app });

db.write((txn) => {
  const users = txn.collection('users');

  users.insert({ name: 'Alice', age: 31 });
  users.insert({ name: 'Bob', age: 17 });
});

const adults = db.read((txn) => txn.collection('users').find((q) => q.where('age', '>=', 18)));

console.log(adults); // [{ id: 1, name: 'Alice', age: 31 }]
db.close();
```

:::

::: lang dart

```dart
import 'package:darudb/darudb.dart';

part 'main.g.dart';

@Collection('users')
class User {
  const User({this.id, required this.name, this.age = 0});

  final int? id;
  final String name;
  @Index()
  final int age;
}

void main() {
  final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

  db.write((txn) {
    final users = txn.collection(userSchema);

    users.insert(const User(name: 'Alice', age: 31));
    users.insert(const User(name: 'Bob', age: 17));
  });

  final adults = db.read(
    (txn) => txn.collection(userSchema).find((q) => q.where(q.age.atLeast(18))),
  );

  print(adults.map((user) => user.name)); // (Alice)
  db.close();
}
```

`dart run build_runner build` writes `main.g.dart`, which holds `userSchema` and the query builder whose `q.age` the query uses.

:::

The collection has no primary key field, so the engine gives each object an `id`, numbered from 1. The index on `age` lets the query read only the objects it finds rather than every object.

## Options

::: lang rust

`OpenOptions` sets everything about opening. Each method returns the builder, and `open` opens the file:

- `create(false)` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `page_size` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cache_size` sets how much memory, in bytes, the page cache may take: 32 MiB by default.
- `busy_timeout` sets how long a write waits for another writer before failing with `BUSY`: five seconds by default.
- `max_unsynced_pages` and `max_unsynced_time` limit how much deferred commits may leave unsynced. See [Transactions](./transactions.md).
- `key`, `password` and `password_hashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migration` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).

[`OpenOptions`](../api/rust/open-options.md) in the API section has each of them in full.

:::

::: lang node

`Database.open` takes an options object as its second argument:

- `create: false` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `pageSize` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cacheSize` sets how much memory, in bytes, the page cache may take: 32 MiB by default.
- `busyTimeout` sets how long, in milliseconds, a write waits for another writer before failing with `BUSY`: 5000 by default.
- `key`, `password` and `passwordHashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migrations` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).

[`OpenOptions`](../types/node/open-options.md) in the Types section has each of them in full.

:::

::: lang dart

`Database.open` takes named options:

- `create: false` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `pageSize` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cacheSize` sets how much memory, in bytes, the page cache may take: 32 MiB by default.
- `busyTimeout`, a `Duration`, sets how long a write waits for another writer before failing with `BUSY`: five seconds by default.
- `key`, `password` and `passwordHashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migrations` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).

[`Database.open`](../api/dart/database.md#open) in the API section has each of them in full, and `Database.openAsync` takes the same.

:::

## Next steps

- [Collections and objects](./objects.md) declares a schema and reads and writes objects.
- [Queries](./queries.md) finds objects by their fields, in code or as text.
- [Transactions](./transactions.md) explains what a commit promises, and when to defer one.
- [Errors](./errors.md) lists every error code.
- [Migration](../migration/index.md) moves an application's data in from the embedded database it uses now.
