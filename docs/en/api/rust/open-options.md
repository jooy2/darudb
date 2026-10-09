---
title: OpenOptions
order: 2
counterpart: /types/node/open-options
---

# OpenOptions

`OpenOptions` holds the options a database is opened with: whether to create it, its page size, how long to wait, how much memory to use, its key or password, and its schema.

```rust
#[derive(Debug, Clone)]
pub struct OpenOptions
```

It is a builder in the style of `std::fs::OpenOptions`. Each option method takes `&mut self` and returns `&mut Self`, so calls chain from `OpenOptions::new()` and end with [`open`](#open). A chain returns a reference to a temporary value, so options that are kept for later need a binding of their own first. `OpenOptions` also implements `Default`, which is `new`.

```rust
use std::time::Duration;

use darudb::OpenOptions;

fn main() -> darudb::Result<()> {
    let db = OpenOptions::new()
        .create(false)
        .busy_timeout(Duration::from_secs(10))
        .open("app.darudb")?;

    db.close()?;

    let mut options = OpenOptions::new();

    options.password("correct horse battery staple");

    let secret = options.open("secret.darudb")?;

    secret.close()
}
```

Every handle to a file in one process shares one instance, and some options belong to that instance: the busy timeout, the cache size, the two limits on deferred commits, the password hashing cost and whether to upgrade the format of the handle that opened the file first apply to every handle opened after it. The page size applies only when the file is created. A key or password has to be given by every handle to an encrypted file, and each handle has its own schema.

## Defaults

| Option               | Default                               |
| -------------------- | ------------------------------------- |
| `create`             | `true`                                |
| `page_size`          | 4096 bytes                            |
| `busy_timeout`       | 5 seconds                             |
| `cache_size`         | 32 MiB                                |
| `max_unsynced_pages` | 16384 pages                           |
| `max_unsynced_time`  | 1 second                              |
| `password_hashing`   | 19456 KiB, 2 iterations, 1 lane       |
| `upgrade_format`     | `true`                                |
| `key`, `password`    | None: the database is not encrypted   |
| `schema`             | None: the database has no collections |

## Associated functions

### new

```rust
pub fn new() -> Self
```

Options set to the defaults above.

## Methods

### create

```rust
pub fn create(&mut self, create: bool) -> &mut Self
```

Whether to create the database when nothing exists at the path. With it off, opening a path where nothing exists fails with `NOT_FOUND`. An existing file is never replaced either way.

### page_size

```rust
pub fn page_size(&mut self, bytes: u32) -> &mut Self
```

The page size of a new database, in bytes: a power of two from 4096 to 65536. An existing file keeps the page size its header records. Any other value fails `open` with `INVALID_ARGUMENT`, even for a file that exists already.

### busy_timeout

```rust
pub fn busy_timeout(&mut self, timeout: Duration) -> &mut Self
```

How long [`Database::begin_write`](./database.md#begin-write) waits for a write transaction already running, in this process or another, and how long opening waits for another process that is recovering the file, before failing with `BUSY`. The calls that wait for the write transaction, such as `sync`, `close` and `compact`, use it too.

### cache_size

```rust
pub fn cache_size(&mut self, bytes: usize) -> &mut Self
```

How much memory the file's page cache may take, in bytes. The cache keeps pages read from the file, checked and decrypted, so that reading one again costs neither a read nor a check. It holds as many pages as fit in `bytes`, and at least 16, and it fills only as pages are read, so a database smaller than the cache never takes all of it. A larger cache speeds up reading a database that does not fit in it; a process with little memory, such as a mobile app extension, can give it less.

### max_unsynced_pages

```rust
pub fn max_unsynced_pages(&mut self, pages: u64) -> &mut Self
```

How many pages deferred commits may write before one of them is made durable anyway, each page counted once however often they write it: 16384 pages is 64 MiB with 4096-byte pages. The limit bounds how much the barrier that makes them durable has to write, and how much recovery has to check after a power cut. A deferred commit that would pass it is made durable itself.

### max_unsynced_time

```rust
pub fn max_unsynced_time(&mut self, time: Duration) -> &mut Self
```

How long deferred commits may go without a barrier. When the time is up, a thread the engine starts for the purpose makes them durable, as [`Database::sync`](./database.md#sync) would; if a write transaction is running at that moment, the thread waits for it, and a deferred commit made after the time is up is made durable itself. The thread exists only while deferred commits are waiting.

### upgrade_format

```rust
pub fn upgrade_format(&mut self, upgrade: bool) -> &mut Self
```

Whether opening a file in an older format version raises it to [`FORMAT_VERSION`](../../types/rust/constants.md), the newest this build writes. On by default.

- **What it costs.** Raising the version rewrites the header and nothing else, with three barriers, whatever the file holds. The leaves written before keep their layout until a write transaction changes them, when they take the newer, smaller one, and [`Database::compact`](./database.md#compact) rewrites the trees where that saves room.
- **When it happens.** Only while no other process has the file open, in the process that opens it then. With another process in, the file stays as it is until the next such open.
- **Going back.** A build that knows only the older version refuses the file once it is raised. An application that may go back to such a build turns this off, and raises the version with [`Database::upgrade_format`](./database.md#upgrade-format) once it no longer may. With it off, a new database is created in format version 5, which every release reads.

[File format](../../engine/file-format.md#format-versions) says what changed between the versions.

### key

```rust
pub fn key(&mut self, key: [u8; 32]) -> &mut Self
```

Encrypts a new database with `key`, or opens an encrypted one with it. Every page of an encrypted database is encrypted and authenticated under a random data key, which `key` wraps: with XAES-256-GCM when the machine creating the database has AES instructions, and with XChaCha20-Poly1305 otherwise.

- **Errors at open.** An encrypted database opened without a key or password fails with `KEY_REQUIRED`, and with another one with `WRONG_KEY`. A plain database opened with a key fails with `INVALID_ARGUMENT`, and never becomes encrypted: that takes a new file.
- **One secret.** `key` and [`password`](#password) replace each other; the last one given counts.

Keep the key somewhere safe, such as the operating system's keystore. Without it, the data cannot be recovered. [Encryption](../../guide/encryption.md) has the rest.

### password

```rust
pub fn password(&mut self, password: impl AsRef<[u8]>) -> &mut Self
```

Encrypts a new database with a key derived from `password`, or opens an encrypted one with it. The password is hashed with Argon2id, at the cost [`password_hashing`](#password-hashing) sets, into the key that wraps the data key; otherwise it is the same as [`key`](#key). An empty password fails `open` with `INVALID_ARGUMENT`.

### password_hashing

```rust
pub fn password_hashing(
    &mut self,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> &mut Self
```

How much work hashing a password takes when a new database is encrypted with one, or when [`Database::set_password`](./database.md#set-password) changes it: Argon2id memory in KiB, iterations, and parallelism. The default takes tens of milliseconds on a current computer and fits the memory limits of mobile app extensions. More makes guessing the password slower for an attacker and opening the database slower for everyone.

A file records the cost it was made with, so opening it takes that cost whatever these options say. Memory from 8 KiB per lane to 1 GiB, iterations from 1 to 1024 and parallelism from 1 to 64 are accepted; anything else fails `open` with `INVALID_ARGUMENT`.

### schema

```rust
pub fn schema(&mut self, schema: Schema) -> &mut Self
```

Declares the collections of the database, and what their objects hold, at a version; see [`Schema`](./schema.md). The first open stores the schema in the file. Later, a file holding the same version opens if the schema is the same and fails with `SCHEMA_MISMATCH` if it is not, so a changed schema needs a new version. A file holding an older version is migrated before `open` returns, and one holding a newer version fails with `SCHEMA_TOO_NEW`. Without a schema, the database has no collections, and only its trees of bytes are reachable.

[Collections and objects](../../guide/objects.md) shows a schema in use.

### migration

```rust
pub fn migration(&mut self, migration: Migration) -> &mut Self
```

Adds a [`Migration`](./migration.md): what one schema version changes from the one before, beyond what the engine works out by itself. Opening a file holding an older version runs the migrations up to the declared version, in version order, in one write transaction. A migration to a version below 2 or above the declared one, or migrations without a schema, fail `open` with `INVALID_ARGUMENT`.

### open

```rust
pub fn open(&self, path: impl AsRef<Path>) -> Result<Database>
```

Opens the database at `path` with these options, creating it first if nothing is there and `create` allows it. With a schema, it also stores, checks or migrates the schema before returning.

- **`INVALID_ARGUMENT`**: an option out of range, a schema or migration that cannot be stored, or a key for a plain file.
- **`NOT_FOUND`**: nothing at the path, and `create(false)`.
- **`NOT_A_DATABASE`**, **`UNSUPPORTED_FORMAT_VERSION`**, **`CORRUPTED`**: the file is not a DaruDB database, is in another format version, or is damaged.
- **`KEY_REQUIRED`**, **`WRONG_KEY`**: an encrypted file opened without its key or password, or with another.
- **`BUSY`**: another process was recovering the file for longer than the busy timeout, salvage is reading it, or the schema had to be stored or migrated and the write transaction stayed busy that long.
- **`UNSUPPORTED_FILE_SYSTEM`**: the file is on a network file system, or on one whose locks do not work.
- **`SCHEMA_MISMATCH`**, **`SCHEMA_TOO_NEW`**: the schema differs from the stored one at the same version, or the file holds a newer version.
- **Migration errors**: `DUPLICATE_KEY` when a new unique index finds a value twice, and `MIGRATION_FAILED` or any other error a migration function returns. The file then keeps its old schema and data.
- **`IO`**: the operating system failed an operation on the file.

[Errors](../../guide/errors.md) describes every code.

### open_migrating

```rust
pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening>
```

Opens the database like [`open`](#open), except that a migration stops for the caller between its version steps. A language binding runs its migration functions this way, since they cannot be Rust closures; an application uses `open`. [Opening and PendingMigration](./opening.md) describes it.

### salvage

```rust
pub fn salvage(
    &self,
    from: impl AsRef<Path>,
    into: impl AsRef<Path>,
) -> Result<SalvageReport>
```

Rescues what it can of the damaged database at `from` into a new database at `into`, and returns what it rescued and what it could not in a [`SalvageReport`](../../types/rust/salvage-report.md). It reads the file page by page rather than opening it, so it works on a file that does not open. Of these options it uses the key or password, for an encrypted file, and the busy timeout.

It starts from the newest commit the file records, and takes what that commit cannot read from older versions of the same pages where the file still has them. The new file gets every index built again from its objects, so it passes the integrity check, and it has the old file's page size, cipher and key.

It needs the file alone: a file open in this process or another fails with `BUSY`. The new file is durable when this returns, and a file already at `into` is never replaced: that fails with `INVALID_ARGUMENT`. [Tools](../../guide/tools.md) shows when to reach for it.
