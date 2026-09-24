---
title: Getting started
order: 2
---

# Getting started

DaruDB is not published yet, so for now you build it from source and open a first database from Rust or Node.js.

## Requirements

- **Rust**, installed with [rustup](https://rustup.rs). The repository pins the compiler version in `rust-toolchain.toml`, and `rustup` installs that version on the first build. A program that depends on the `darudb` crate needs Rust 1.85 or later.
- **Node.js 20 or later**, for the Node.js package. Building it from source needs a little more, because of its build tool: 20.17 or later on the 20 line, or 22.13 or later.
- **Git**, to clone the repository.

DaruDB runs on Unix-like systems and on Windows. Network file systems such as NFS and SMB are not supported, because their file locks and syncs do not keep the promises a database relies on.

## Build from source

```bash
git clone https://github.com/jooy2/darudb.git
cd darudb
cargo test --workspace
```

`cargo test` builds the engine and runs its tests, which is the quickest way to know your toolchain works.

For the Node.js package, build the native addon in its folder:

```bash
cd packages/node
npm install
npm run build
```

`npm run build` compiles the engine and the binding into one addon for your platform, and writes the files that load it next to it.

## Open a database

A database is one file. Opening a path where nothing exists creates the file; opening an existing file checks that it is a DaruDB database this version can read.

### Rust

Add the crate by path until it is published:

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb" }
```

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

The storage kernel stores named trees of byte keys and byte values. Every change goes through a write transaction and becomes visible, and durable, together when `commit` returns. A read transaction sees one commit for as long as it lives, whatever is committed after it began.

```rust
use darudb::Database;

fn main() -> Result<(), darudb::Error> {
    let db = Database::open("app.darudb")?;

    let mut txn = db.begin_write()?;
    txn.insert("users", b"alice", b"admin")?;
    txn.insert("users", b"bob", b"member")?;
    txn.commit()?;

    let read = db.begin_read()?;
    assert_eq!(read.get("users", b"alice")?, Some(b"admin".to_vec()));

    // Keys come back in byte order.
    for entry in read.range("users", b"a".as_slice()..b"c".as_slice())? {
        let (key, value) = entry?;
        println!("{} = {}", String::from_utf8_lossy(&key), String::from_utf8_lossy(&value));
    }

    Ok(())
}
```

`range_backward` walks the same keys from the last down, which is how to read the newest entries of a tree whose keys grow.

A write transaction dropped without `commit` is aborted, and nothing it did reaches the file. There is one write transaction at a time; `begin_write` waits for the one already running for up to the busy timeout, five seconds unless `OpenOptions::busy_timeout` says otherwise.

`commit` waits for the disk before it returns. `commit_deferred` does not: readers see the changes at once, and they reach the disk together with later commits, at the next `commit`, at `Database::sync`, when the database is closed, or once they have waited one second by default. A crash of the process loses none of them. A power cut can undo the newest ones, but never leaves a gap and never damages the file. `OpenOptions::max_unsynced_time` and `OpenOptions::max_unsynced_pages` set how much may wait.

Several processes can have one file open at once. Each sees the others' commits as soon as they are made, one writes at a time, and a reader never waits for a writer. `begin_write` waits for a writer in another process as it does for one in its own, up to the busy timeout. The processes coordinate through the operating system's file locks and nothing else, so a process that dies at any moment leaves nothing the others have to clean up. Those locks, and the syncs a commit waits for, work only on a local disk: a database on a network file system such as NFS or SMB is refused with `UNSUPPORTED_FILE_SYSTEM`. Two more rules come with the locks. Nothing else in a process that has a database open may open the file, not even to copy it: on Linux and macOS, closing that second handle drops the locks the database holds. And on iOS, an app whose database lives in an App Group container has to close it before the app is suspended, because iOS ends a suspended app that holds a lock there.

Each process keeps the pages it reads in a cache, so that reading a page again costs neither a read nor a check. The cache takes up to 32 MiB for each open file by default, and only as pages are read, so a smaller database never takes all of it. `OpenOptions::cache_size` sets the size in bytes: more for a large database that is read often, less in a process with little memory, such as a mobile app extension.

Opened with a schema, a database also holds collections of typed objects, kept in trees of the engine's own that `tree_names` does not list. [Collections and objects](./objects.md) shows how.

#### Encryption

A database created with a key or a password is encrypted: every page, keys, values and tree names included. Every page is authenticated, and so is the header's record of each commit, so a changed byte is reported as `CORRUPTED` rather than read.

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    db.set_password("a new password")?;
    db.close()
}
```

Pages are encrypted with XAES-256-GCM on processors with AES instructions and with XChaCha20-Poly1305 elsewhere, whichever is faster on the machine that creates the database. `OpenOptions::key` takes a 32-byte key instead of a password, such as one kept in the operating system's keystore. A password is hashed with Argon2id, which takes tens of milliseconds by default; `OpenOptions::password_hashing` raises or lowers that cost. Changing the key or the password re-encrypts nothing, and once it returns, the old one no longer opens the file. A plain database stays plain, and an encrypted one cannot be opened without its key: keep it where it cannot be lost.

### Node.js

```js
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();
```

`Database.open` takes an options object as its second argument: `create: false` refuses to create a missing file, `pageSize` sets the page size of a new one, and `cacheSize` the memory the page cache may take, in bytes. With a `schema`, the database holds collections of objects that transactions read and write and queries find: [Node.js](./nodejs.md) shows how.

## Errors

Every error carries a `code` that names the failure. The code is the same in Rust (`Error::code`) and in Node.js (`error.code`), and it does not change between releases, so a program can rely on it where the message is meant for a person.

| Code | When |
| --- | --- |
| `NOT_FOUND` | Nothing exists at the path, and creating a database was not allowed. |
| `NOT_A_DATABASE` | The file exists but is not a DaruDB database. |
| `UNSUPPORTED_FORMAT_VERSION` | The file is a DaruDB database in a format version this build cannot read: a newer build wrote it. |
| `CORRUPTED` | The file is a DaruDB database, but part of it has been damaged. |
| `INVALID_ARGUMENT` | An option was out of range, such as a page size that is not a power of two, or an object does not fit the schema. |
| `CLOSED` | A Node.js database object was used after `close`. |
| `BUSY` | The database stayed busy for longer than the busy timeout: another write transaction held it, or another process was recovering it. |
| `SYNC_FAILED` | A sync of the file failed. The last commit may or may not have happened; open the file again. |
| `KEY_REQUIRED` | The database is encrypted, and it was opened without a key or password. |
| `WRONG_KEY` | The key or password does not open the database. |
| `UNSUPPORTED_FILE_SYSTEM` | The database is on a network file system, or on one whose file locks do not work. It has to be on a local disk. |
| `SCHEMA_MISMATCH` | The declared schema differs from the one the file holds at the same version, or the file was migrated since this handle opened it. |
| `SCHEMA_TOO_NEW` | The file holds a newer schema version than the one declared: a newer application wrote it. |
| `DUPLICATE_KEY` | An insert found its primary key taken, or a unique index found a value taken. |
| `INVALID_QUERY` | A query names a field the collection does not have, or tests one with a value of another type. |
| `MIGRATION_FAILED` | A migration function reported that it failed. The file keeps its old schema and data. |
| `INTERNAL` | Something only a bug in DaruDB can cause. Please report it. |
| `IO` | The operating system failed an operation on the file. The message says what it reported. |

```js
try {
  Database.open('missing.darudb', { create: false });
} catch (error) {
  if (error.code === 'NOT_FOUND') {
    // Nothing exists at that path.
  }
}
```
