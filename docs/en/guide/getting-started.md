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

`npm run build` compiles the engine and the binding into one addon for your platform, and writes `index.js` and `index.d.ts` next to it.

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

A write transaction dropped without `commit` is aborted, and nothing it did reaches the file. There is one write transaction at a time; `begin_write` waits for the one already running for up to the busy timeout, five seconds unless `OpenOptions::busy_timeout` says otherwise.

`commit` waits for the disk before it returns. `commit_deferred` does not: readers see the changes at once, and they reach the disk together with later commits, at the next `commit`, at `Database::sync`, when the database is closed, or once they have waited one second by default. A crash of the process loses none of them. A power cut can undo the newest ones, but never leaves a gap and never damages the file. `OpenOptions::max_unsynced_time` and `OpenOptions::max_unsynced_pages` set how much may wait.

Typed records and queries come later, built on top of these trees. For now, only one process may have a file open at a time.

### Node.js

```js
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();
```

`Database.open` takes an options object as its second argument: `create: false` refuses to create a missing file, and `pageSize` sets the page size of a new one. Transactions are not in the Node.js package yet; they reach it once the engine's API has settled.

## Errors

Every error carries a `code` that names the failure. The code is the same in Rust (`Error::code`) and in Node.js (`error.code`), and it does not change between releases, so a program can rely on it where the message is meant for a person.

| Code | When |
| --- | --- |
| `NOT_FOUND` | Nothing exists at the path, and creating a database was not allowed. |
| `NOT_A_DATABASE` | The file exists but is not a DaruDB database. |
| `UNSUPPORTED_FORMAT_VERSION` | The file is a DaruDB database in a file format version this build cannot read. |
| `CORRUPTED` | The file is a DaruDB database, but part of it has been damaged. |
| `INVALID_ARGUMENT` | An option was out of range, such as a page size that is not a power of two. |
| `CLOSED` | A Node.js database object was used after `close`. |
| `BUSY` | Another write transaction held the database for longer than the busy timeout. |
| `SYNC_FAILED` | A sync of the file failed. The last commit may or may not have happened; open the file again. |
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
