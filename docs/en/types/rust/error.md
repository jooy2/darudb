---
title: Error and Result
order: 7
counterpart: /types/node/error
---

# Error and Result

`Error` is the failure every fallible call of the crate returns, and `Result` is the crate's result type with that error.

```rust
#[derive(Debug)]
#[non_exhaustive]
pub enum Error

pub type Result<T, E = Error> = std::result::Result<T, E>;
```

Each variant carries a stable code, which [`code`](#code) returns: the same string in every language binding, never renamed once released. A program that reacts to a failure matches on the variant or on the code; the message that `Display` writes is for a person and may be reworded in any release. [Errors](../../guide/errors.md) explains when each code happens and what to do about it.

`darudb::Result<T>` is `std::result::Result<T, Error>`. Its second parameter lets a function that returns another error type still use the alias.

`Error` is `#[non_exhaustive]`, so new variants may come in any release and a `match` on it needs a wildcard arm. It implements `std::error::Error` and is `Send` and `Sync`, so `?` can turn it into a `Box<dyn std::error::Error + Send + Sync>`, or into an application's own error type that implements `From<darudb::Error>`.

```rust
use darudb::{Database, Error};

fn open_now(path: &str) -> darudb::Result<Option<Database>> {
    match Database::open(path) {
        Ok(db) => Ok(Some(db)),
        Err(Error::Busy { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}
```

## Variants

### Io

```rust
Io { path: PathBuf, source: io::Error }
```

`IO`. The operating system failed an operation on the file or its directory. `path` is what the operation was on, and `source` is what the system reported.

### NotFound

```rust
NotFound { path: PathBuf }
```

`NOT_FOUND`. No database exists at the path, and the options did not allow creating one.

### NotADatabase

```rust
NotADatabase { path: PathBuf }
```

`NOT_A_DATABASE`. The file is empty, too short to hold a header, or does not start with DaruDB's bytes.

### UnsupportedFormatVersion

```rust
UnsupportedFormatVersion { path: PathBuf, found: u32, supported: u32 }
```

`UNSUPPORTED_FORMAT_VERSION`. The file is a DaruDB database in a format this build does not read. `found` is the version the file records and `supported` the one this build reads: the file format version, [`FORMAT_VERSION`](./constants.md#format-version), or the object format version of the schema stored in the file.

### Corrupted

```rust
Corrupted { path: PathBuf, reason: String }
```

`CORRUPTED`. What the file records is impossible, so part of it is damaged. `reason` says what was found.

### InvalidArgument

```rust
InvalidArgument { message: String }
```

`INVALID_ARGUMENT`. The caller asked for something that cannot be done, such as an option out of range, a schema that breaks a rule, an object that does not fit its collection, or a path a tool will not write over.

### Closed

```rust
Closed
```

`CLOSED`. The database was used after it was closed. The Rust API never returns it, since [`Database::close`](../../api/rust/database.md#close) consumes the handle; it exists so that the bindings, whose handles outlive their close, report that case with a code from this list.

### Busy

```rust
Busy { path: PathBuf }
```

`BUSY`. The database stayed busy for longer than the busy timeout: another write transaction held it, in this process or another, or another process was recovering it. Salvage fails with it when the file is open, and opening a file that salvage is reading fails with it too.

### SyncFailed

```rust
SyncFailed { path: PathBuf, source: Option<io::Error> }
```

`SYNC_FAILED`. A barrier failed, so the outcome of a commit is unknown. From then on every handle to the file in this process fails with it, until all of them are gone and the file is opened again. `source` is what the operating system reported on the failure itself, and `None` on a later use of the database.

### KeyRequired

```rust
KeyRequired { path: PathBuf }
```

`KEY_REQUIRED`. The database is encrypted, and it was opened without a key or a password.

### WrongKey

```rust
WrongKey { path: PathBuf }
```

`WRONG_KEY`. The key or password does not open the database.

### UnsupportedFileSystem

```rust
UnsupportedFileSystem { path: PathBuf }
```

`UNSUPPORTED_FILE_SYSTEM`. The database is on a network file system, or on one whose file locks do not work. A database has to be on a local disk.

### SchemaMismatch

```rust
SchemaMismatch { message: String }
```

`SCHEMA_MISMATCH`. The declared schema differs from the one the file holds at the same version, or another handle or process migrated the file since this handle opened it. `message` says what differs.

### SchemaTooNew

```rust
SchemaTooNew { stored: u64, declared: u64 }
```

`SCHEMA_TOO_NEW`. The file holds a newer schema version, `stored`, than the application declared, `declared`.

### DuplicateKey

```rust
DuplicateKey { message: String }
```

`DUPLICATE_KEY`. An insert found its primary key taken, or a unique index found its value taken. `message` names the key or value and the collection.

### InvalidQuery

```rust
InvalidQuery { message: String }
```

`INVALID_QUERY`. A query does not parse, or does not fit the schema. `message` says what is wrong and where.

### MigrationFailed

```rust
MigrationFailed { message: String }
```

`MIGRATION_FAILED`. A migration function reported an error. The engine never makes this one: a migration function returns it to say why it stopped, while an engine error it passes on with `?` keeps its own code. Either way the file keeps its old schema and data.

```rust
use darudb::{Error, Migration};

let migration = Migration::to(2).run(|migrating| {
    if migrating.collection("users")?.len()? > 1_000_000 {
        return Err(Error::MigrationFailed {
            message: "too many users to migrate at startup".to_owned(),
        });
    }

    Ok(())
});
```

### Internal

```rust
Internal { message: String }
```

`INTERNAL`. An invariant of the engine does not hold, which only a bug in DaruDB can cause. Please report it with `message`.

## Methods

### code

```rust
pub fn code(&self) -> &'static str
```

The stable, machine-readable name of the failure, in `SCREAMING_SNAKE_CASE`: the code under each variant above. The Node.js package exposes the same string as the error's `code`.

```rust
use darudb::{Object, WriteTransaction};

fn add_tag(txn: &mut WriteTransaction, name: &str) -> darudb::Result<bool> {
    match txn.collection("tags")?.insert(Object::new().with("name", name)) {
        Ok(_) => Ok(true),
        Err(error) if error.code() == "DUPLICATE_KEY" => Ok(false),
        Err(error) => Err(error),
    }
}
```

## Traits

- **`Display`** writes a message for a person, which names the file in every variant that has a `path`.
- **`std::error::Error`** gives the operating system's error as the `source` of `Io`, and of `SyncFailed` when it has one, and no source for the other variants.
- **`Debug`** prints the variant with its fields.
