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

### Node.js

```js
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();
```

`Database.open` takes an options object as its second argument: `create: false` refuses to create a missing file, and `pageSize` sets the page size of a new one.

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
