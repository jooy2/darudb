---
title: Getting started
order: 2
---

# Getting started

This page adds DaruDB to a project from its language's registry, crates.io, npm, pub.dev or PyPI, and opens a first database.

## Requirements

::: lang rust

- **Rust 1.85 or later**, installed with [rustup](https://rustup.rs).

:::

::: lang node

- **Node.js 20 or later.** The package ships prebuilt addons for macOS, Windows, Linux (glibc and musl), FreeBSD and Android, so installing it compiles nothing.

:::

::: lang dart

- **Dart 3.10 or later**, or Flutter 3.38.1 or later: the package provides its native library through a build hook, which those releases made stable. Flutter 3.38.0 ships a beta build of Dart 3.10, which the package does not accept.
- **Network access the first time an application builds for a target.** The build hook downloads the engine prebuilt for that target from the package's GitHub release, checks its SHA-256 hash and keeps it in its cache, so building needs no Rust toolchain. Libraries ship for Android, iOS, macOS, Windows and Linux. An application that needs a static library, or a target without a prebuilt one, depends on the package through git instead, which builds the engine from source with [rustup](https://rustup.rs).

:::

::: lang python

- **CPython 3.11 or later.** The package ships prebuilt wheels for Linux (glibc 2.17 or later, and musl), macOS and Windows, on x86-64 and ARM64, and for 32-bit Windows. One wheel serves every CPython from 3.11, and the free-threaded build of Python 3.14 has wheels of its own, so installing the package compiles nothing.

:::

DaruDB runs on Unix-like systems and on Windows. Network file systems such as NFS and SMB are not supported, because their file locks and syncs do not keep the promises a database relies on.

## Add it to a project

::: lang rust

```bash
cargo add darudb
```

The `derive` feature adds `#[derive(Object)]`, which makes a struct the objects of a collection; see [Collections and objects](./objects.md#objects-as-rust-types). Add the crate with it instead:

```bash
cargo add darudb --features derive
```

:::

::: lang node

```bash
npm install darudb
```

The package is TypeScript-first: its declarations ship with it, and the types of your objects follow from the schema you declare. It works from JavaScript as well.

:::

::: lang dart

Add the package, with the generator that writes the code for your classes:

```yaml
dependencies:
  darudb: ^1.6.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.6.0
```

The package works in Flutter apps, Dart servers and command-line tools alike. [Collections and objects](./objects.md) shows the classes the generator reads.

:::

::: lang python

```bash
pip install darudb
```

The package ships its type hints, and each collection is a class you declare as a dataclass, so a type checker knows the fields of every object you read.

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

The examples on this page use the synchronous API, which returns its result without `await`. Every call that uses the file also has a twin whose name ends in `Async` and returns a promise. A server or Electron's main process should use it for writes and for opening, so that a wait for the disk does not hold up the event loop. [Which one to use](./async.md#which-one-to-use) says where each belongs.

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

The examples on this page use the synchronous API, which returns its result without `await`. Every call that uses the file also has a twin whose name ends in `Async` and returns a `Future`. A Flutter app's UI isolate should use it for writes and for opening, so that a wait for the disk does not hold up its frames. [Which one to use](./async.md#which-one-to-use) says where each belongs.

:::

::: lang python

```python
import darudb

db = darudb.Database.open("app.darudb")

print(f"page size: {db.page_size} bytes")
db.close()

# Open only if the file is already there.
darudb.Database.open("app.darudb", create=False).close()
```

A database is also a context manager: `with darudb.Database.open("app.darudb") as db:` closes it when the block ends.

The examples on this page use the synchronous API. Every call that uses the file also has a twin whose name ends in `_async`, for `asyncio`. A program that runs on an event loop, such as an `asyncio` server, should use it for writes and for opening, so that a wait for the disk does not hold up the loop. [Which one to use](./async.md#which-one-to-use) says where each belongs.

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

On Dart 3.10, `build_runner` stops with `'dart compile' does not support build hooks`. The newest `build_runner` that runs on Dart 3.10 compiles the builders ahead of time with `dart compile`, which Dart 3.10 refuses for a project whose packages have a build hook, as `darudb` has. Run `dart run build_runner build --force-jit` there, which compiles the builders as they run instead.

:::

::: lang python

```python
import darudb
from darudb import F, field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    age: int = field(default=0, index=True)


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))

with db.write() as txn:
    users = txn.collection(User)

    users.insert(User(name="Alice", age=31))
    users.insert(User(name="Bob", age=17))

with db.read() as txn:
    adults = txn.collection(User).find(F.age >= 18)

print(adults)  # [User(id=1, name='Alice', age=31)]
db.close()
```

`@darudb.collection` makes the class a frozen dataclass whose annotations are the types of its fields. The `write` block commits when it ends, and the objects a query finds are instances of the class.

:::

The collection has no primary key field, so the engine gives each object an `id`, numbered from 1. The index on `age` lets the query read only the objects it finds rather than every object.

## Options

::: lang rust

`OpenOptions` sets everything about opening. Each method returns the builder, and `open` opens the file:

- `create(false)` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `page_size` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cache_size` sets how much memory, in bytes, the page cache may take: 128 MiB by default and 256 MiB at most, or 32 MiB for both on iOS and Android.
- `busy_timeout` sets how long a write waits for another writer before failing with `BUSY`: five seconds by default.
- `max_unsynced_pages` and `max_unsynced_time` limit how much deferred commits may leave unsynced. See [Transactions](./transactions.md).
- `key`, `password` and `password_hashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migration` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).
- `upgrade_format(false)` keeps a file in an older format version as it is, for an application that may go back to a release that reads only that version. See [Format versions](../engine/file-format.md#format-versions).
- `read_only(true)` opens the database read-only, so that nothing done through it changes the file. See [Open a database read-only](#open-a-database-read-only).

[`OpenOptions`](../api/rust/open-options.md) in the API section has each of them in full.

:::

::: lang node

`Database.open` takes an options object as its second argument:

- `create: false` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `pageSize` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cacheSize` sets how much memory, in bytes, the page cache may take: 128 MiB by default and 256 MiB at most, or 32 MiB for both on iOS and Android.
- `busyTimeout` sets how long, in milliseconds, a write waits for another writer before failing with `BUSY`: 5000 by default.
- `maxUnsyncedPages` and `maxUnsyncedTime`, in milliseconds, limit how much deferred commits may leave unsynced. See [Transactions](./transactions.md).
- `key`, `password` and `passwordHashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migrations` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).
- `upgradeFormat: false` keeps a file in an older format version as it is, for an application that may go back to a release that reads only that version. See [Format versions](../engine/file-format.md#format-versions).
- `readOnly: true` opens the database read-only, so that nothing done through it changes the file. See [Open a database read-only](#open-a-database-read-only).

[`OpenOptions`](../types/node/open-options.md) in the Types section has each of them in full.

:::

::: lang dart

`Database.open` takes named options:

- `create: false` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `pageSize` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cacheSize` sets how much memory, in bytes, the page cache may take: 128 MiB by default and 256 MiB at most, or 32 MiB for both on iOS and Android.
- `busyTimeout`, a `Duration`, sets how long a write waits for another writer before failing with `BUSY`: five seconds by default.
- `maxUnsyncedPages` and `maxUnsyncedTime`, a `Duration`, limit how much deferred commits may leave unsynced. See [Transactions](./transactions.md).
- `key`, `password` and `passwordHashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migrations` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).
- `upgradeFormat: false` keeps a file in an older format version as it is, for an application that may go back to a release that reads only that version. See [Format versions](../engine/file-format.md#format-versions).
- `readOnly: true` opens the database read-only, so that nothing done through it changes the file. See [Open a database read-only](#open-a-database-read-only).

[`Database.open`](../api/dart/database.md#open) in the API section has each of them in full, and `Database.openAsync` takes the same.

:::

::: lang python

`Database.open` takes keyword options:

- `create=False` refuses to create a missing file, which then fails with `NOT_FOUND`.
- `page_size` sets the page size of a new file: a power of two from 4096 to 65536, 4096 by default.
- `cache_size` sets how much memory, in bytes, the page cache may take: 128 MiB by default and 256 MiB at most, or 32 MiB for both on iOS and Android.
- `busy_timeout` sets how long, in seconds, a write waits for another writer before failing with `BUSY`: 5 by default. It takes a `float`, so `0.5` is half a second.
- `max_unsynced_pages` and `max_unsynced_time`, in seconds, limit how much deferred commits may leave unsynced. See [Transactions](./transactions.md).
- `key`, `password` and `password_hashing` encrypt a new file or open an encrypted one. See [Encryption](./encryption.md).
- `schema` and `migrations` declare the collections and how an older schema becomes this one. See [Collections and objects](./objects.md) and [Migrations](./migrations.md).
- `upgrade_format=False` keeps a file in an older format version as it is, for an application that may go back to a release that reads only that version. See [Format versions](../engine/file-format.md#format-versions).
- `read_only=True` opens the database read-only, so that nothing done through it changes the file. See [Open a database read-only](#open-a-database-read-only).

[`Database.open`](../api/python/database.md#open) in the API section has each of them in full, and `Database.open_async` takes the same.

:::

## Open a database read-only

A database opened read-only is one through which the file's bytes never change. It is for a program that only looks: a report, a tool that inspects a file another program owns, or a part of an application that must not write by accident.

::: lang rust

```rust
use darudb::OpenOptions;

fn count(path: &str) -> darudb::Result<u64> {
    let db = OpenOptions::new().read_only(true).open(path)?;
    let read = db.begin_read()?;

    read.collection("users")?.len()
}
```

:::

::: lang node

```ts
import { Database } from 'darudb';

const db = Database.open('app.darudb', { readOnly: true });
const users = db.read((txn) => txn.collection('users').count());

db.close();
```

:::

::: lang dart

```dart
final db = Database.open('app.darudb', readOnly: true);
final users = db.read(
  (txn) => txn.collection(db.storedCollection('users')).count(),
);

db.close();
```

:::

::: lang python

```python
import darudb

with darudb.Database.open("app.darudb", read_only=True) as db:
    with db.read() as txn:
        users = txn.collection("users").count()
```

:::

- **What it does.** It reads, checks the file and backs it up. Other handles to the file, in the process or in another, write as ever, and the read-only one reads what they commit.
- **What it refuses.** A write transaction, a schema the file does not hold already, since storing one or migrating to it writes, raising the format version, compaction and a change of key fail with `READ_ONLY`. A sync and a close do nothing to the file. Nothing is created either: a path with no database fails with `NOT_FOUND`.
- **A file that needs recovery.** A file left by a process that died, or by a power cut, with nobody having opened it since, may need recovery before it can be read, and recovery writes to it. A read-only open of such a file fails with `READ_ONLY` and leaves the file as it was. Open it once without the option, and it opens read-only from then on.
- **Write access all the same.** The file is still opened for writing, because the locks that coordinate processes need that. The process has to be allowed to write to the file, and a database on a read-only medium does not open.

<LangCode rust="Database::is_read_only" node="isReadOnly" dart="isReadOnly" python="is_read_only" /> says whether a database was opened this way.

## Next steps

- [Collections and objects](./objects.md) declares a schema and reads and writes objects.
- [Queries](./queries.md) finds objects by their fields, in code or as text.
- [Transactions](./transactions.md) explains what a commit promises, and when to defer one.
- [Errors](./errors.md) lists every error code.
- [DaruDB Studio](./studio.md) opens the file your program wrote, to see and edit its objects.
- [Migration](../migration/index.md) moves an application's data in from the embedded database it uses now.
