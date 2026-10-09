---
title: Database
order: 1
---

# Database

`Database` is a handle to an open database file, from which every transaction begins.

```rust
#[derive(Debug, Clone)]
pub struct Database
```

A program gets one from `Database::open`, or from [`OpenOptions::open`](./open-options.md#open) when it needs options. Cloning a handle, or opening the same file again in the same process, gives another handle to one shared instance: one file handle, one page cache, and one write transaction at a time. The file is closed when the last handle is dropped, and deferred commits are made durable then; [`close`](#close) does the same and reports a failure. `Database` is `Send` and `Sync`, so handles can be cloned into other threads or shared between them.

Each handle keeps the schema it was opened with, and its transactions reach collections through that schema. When another handle or another process migrates the file, the old handle's next transaction to reach a collection fails with `SCHEMA_MISMATCH`, and the file has to be opened again with the new schema.

## Associated functions

### open

```rust
pub fn open(path: impl AsRef<Path>) -> Result<Self>
```

Opens the database at `path`, creating it if nothing exists there. It is `OpenOptions::new().open(path)`, and fails as [`OpenOptions::open`](./open-options.md#open) does.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;

    println!("{} bytes per page", db.page_size());
    db.close()
}
```

## Methods

### path

```rust
pub fn path(&self) -> &Path
```

The path the database was opened at. Handles that share an instance share this path too: the one the file was first opened at in the process.

### page_size

```rust
pub fn page_size(&self) -> u32
```

The size of every page in the file, in bytes. It is fixed when the file is created, by [`OpenOptions::page_size`](./open-options.md#page-size).

### format_version

```rust
pub fn format_version(&self) -> u32
```

The file format version of the file. A build reads and writes one version and refuses a file in any other when it opens it, so this is always [`FORMAT_VERSION`](../../types/rust/constants.md).

### begin_read

```rust
pub fn begin_read(&self) -> Result<ReadTransaction>
```

Starts a [read transaction](./read-transaction.md), which sees the last commit and nothing committed after it. It does not wait for a write transaction, and any number of read transactions may be open at once, in any number of threads and processes. It fails with `SYNC_FAILED` once a barrier on the file has failed.

### begin_write

```rust
pub fn begin_write(&self) -> Result<WriteTransaction>
```

Starts the [write transaction](./write-transaction.md). There is one at a time per file, across every thread and process, so it waits for one already running and fails with `BUSY` once the busy timeout has passed ([`OpenOptions::busy_timeout`](./open-options.md#busy-timeout), five seconds by default). A thread that already holds the write transaction and calls it again waits for itself and fails the same way. It fails with `SYNC_FAILED` once a barrier on the file has failed.

### sync

```rust
pub fn sync(&self) -> Result<()>
```

Makes every commit so far durable, the deferred ones included, whichever process made them. It returns at once when no deferred commit is waiting; otherwise it waits for a running write transaction, in this process or another, and fails with `BUSY` once the busy timeout has passed. A barrier that fails is reported as `SYNC_FAILED`, and the file has to be opened again. [Transactions](../../guide/transactions.md) explains deferred commits.

### is_encrypted

```rust
pub fn is_encrypted(&self) -> bool
```

Whether the database is encrypted. A database is encrypted when it is created with a key or a password, and stays as it was created.

### set_key

```rust
pub fn set_key(&self, key: [u8; 32]) -> Result<()>
```

Changes the key of an encrypted database to `key`. The data key is wrapped again and no page is encrypted again, so it takes three sync commits whatever the size of the file: one that writes the new key block, and two that overwrite the old one in the other commit slots. When it returns, the old key or password no longer opens the file.

A plain database fails with `INVALID_ARGUMENT`: encrypting one takes a new file. It waits for the write transaction as `begin_write` does. [Encryption](../../guide/encryption.md) has the rest.

### set_password

```rust
pub fn set_password(&self, password: impl AsRef<[u8]>) -> Result<()>
```

Changes the key of an encrypted database to one derived from `password` with Argon2id, at the cost that [`OpenOptions::password_hashing`](./open-options.md#password-hashing) gave the handle that opened the file first in this process. An empty password fails with `INVALID_ARGUMENT`. Otherwise it is the same as [`set_key`](#set-key).

### check

```rust
pub fn check(&self) -> Result<CheckReport>
```

Checks the published commit completely: every page it reaches against the check its parent recorded, the order of every key, every tree's count, that every page of the file is used, free or retained exactly once, and, in a file with a schema, every object against its indexes. It reports every problem in a [`CheckReport`](../../types/rust/check-report.md) rather than stopping at the first, and fails only when it cannot begin.

It reads the whole file, from the commit a read transaction would see, so other handles and processes may write while it runs. [Tools](../../guide/tools.md) explains what it reads.

### backup

```rust
pub fn backup(&self, path: impl AsRef<Path>) -> Result<BackupReport>
```

Writes a copy of the published commit to a new file at `path`, and returns a [`BackupReport`](../../types/rust/backup-report.md). Other handles and processes may read and write meanwhile: the copy holds the commit that was published when it began.

The copy has this file's page size, and in an encrypted file its cipher and key, so the same key or password opens it. It holds no free space. It stays under a temporary name beside `path` until it is durable, and only then takes `path`. A file already at `path` is never replaced: that fails with `INVALID_ARGUMENT`.

### backup_with

```rust
pub fn backup_with(&self, path: impl AsRef<Path>, options: &BackupOptions) -> Result<BackupReport>
```

Writes a copy as [`backup`](#backup) does, as [`BackupOptions`](./backup-options.md) say: with a key or a password there, the copy is encrypted under a new random data key, which it wraps, and opens only with it. A plain database's copy is encrypted the same way. An empty password fails with `INVALID_ARGUMENT` before anything is written.

### compact

```rust
pub fn compact(&self) -> Result<CompactReport>
```

Makes the file smaller in place, and returns a [`CompactReport`](../../types/rust/compact-report.md). It writes again, full, every tree whose pages inserts left part empty, so that the file ends about as small as a backup's copy, then moves every page the file's tail holds into free pages below it, in write transactions of its own, and the commits after them give the tail back to the file system.

Other handles and processes may read and write meanwhile. A page that a read transaction can still reach stays until the transaction ends, so a file with long readers shrinks less, and the next compaction takes the rest. It waits for the write transaction as `begin_write` does.

### close

```rust
pub fn close(self) -> Result<()>
```

Closes this handle, making deferred commits durable first. It fails with `SYNC_FAILED` if a barrier failed on any handle to this file, which is the last chance to notice, and with `BUSY` if it waited for a running write transaction longer than the busy timeout. Dropping the handle instead also makes deferred commits durable when it is the last one, but cannot report a failure.

### schema_record

```rust
pub fn schema_record(&self) -> Option<&[u8]>
```

The schema this handle opened the file with, encoded as the file stores it, or `None` for a handle opened without a schema. It is for language bindings, which read the ids of collections and fields from it to encode and decode records themselves; [Bindings](../../engine/bindings.md) explains how they use it, and [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-stored-schema) specifies the encoding. A Rust program has no need of it.
