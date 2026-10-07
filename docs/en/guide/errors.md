---
title: Errors
order: 10
---

# Errors

Every error DaruDB returns carries a `code` that names the failure, the same in every language and stable across releases.

## Read the code

::: lang rust

Every fallible call returns `darudb::Result`, whose error is `darudb::Error`. `Error::code` gives its code as a string; the variants are matched the usual way.

```rust
use darudb::{Error, OpenOptions};

fn open_existing() -> Result<(), Error> {
    match OpenOptions::new().create(false).open("missing.darudb") {
        Ok(db) => db.close(),
        Err(error) if error.code() == "NOT_FOUND" => {
            // Nothing exists at that path.
            Ok(())
        }
        Err(error) => Err(error),
    }
}
```

[`Error`](../types/rust/error.md) in the Types section lists its variants.

:::

::: lang node

Every error the package throws is an `Error` whose `code` is one of the codes below. The asynchronous API rejects with the same errors.

```ts
try {
  Database.open('missing.darudb', { create: false });
} catch (error) {
  if (error.code === 'NOT_FOUND') {
    // Nothing exists at that path.
  }
}
```

[`Error`](../types/node/error.md) in the Types section has the details.

:::

::: lang dart

Every error the package throws for the database is a `DaruException`, whose `code` is one of the codes below. The `Future` API completes with the same errors.

```dart
try {
  Database.open('missing.darudb', create: false);
} on DaruException catch (error) {
  if (error.code == 'NOT_FOUND') {
    // Nothing exists at that path.
  }
}
```

An error a function of yours throws inside a transaction or a migration is thrown again as it was. [`DaruException`](../types/dart/error.md) in the Types section has the details.

:::

::: lang python

Every error the package raises for the database is a `darudb.DaruError`, whose `code` is one of the codes below and whose `message` says what happened. The asynchronous API raises the same errors.

```python
import darudb

try:
    darudb.Database.open("missing.darudb", create=False)
except darudb.DaruError as error:
    if error.code == "NOT_FOUND":
        pass  # Nothing exists at that path.
```

A Python value the engine cannot hold, such as an `int` beyond 64 bits, fails with `INVALID_ARGUMENT` too, rather than a `TypeError`, so one `except` catches every refusal. An exception your own code raises inside a `with` block or a migration's `run` goes on as it was. [`DaruError`](../types/python/error.md) in the Types section has the details.

:::

A program can rely on the code, where the message is meant for a person and may change.

## Every code

| Code | When |
| --- | --- |
| `NOT_FOUND` | Nothing exists at the path, and creating a database was not allowed. |
| `NOT_A_DATABASE` | The file exists but is not a DaruDB database. |
| `UNSUPPORTED_FORMAT_VERSION` | The file is a DaruDB database in a format version this build cannot read: one a newer build wrote, or one a development build wrote before the first release. |
| `CORRUPTED` | The file is a DaruDB database, but part of it has been damaged. |
| `INVALID_ARGUMENT` | An option was out of range, such as a page size that is not a power of two, or an object does not fit the schema. |
| `CLOSED` | A database, a transaction or a collection was used after it was closed or after its transaction ended. |
| `BUSY` | The database stayed busy for longer than the busy timeout: another write transaction held it, or another process was recovering it. Salvage fails with it when the file is open, and so does opening a file salvage is reading. |
| `SYNC_FAILED` | A sync of the file failed. The last commit may or may not have happened; open the file again. |
| `KEY_REQUIRED` | The database is encrypted, and it was opened without a key or password. |
| `WRONG_KEY` | The key or password does not open the database. |
| `UNSUPPORTED_FILE_SYSTEM` | The database is on a network file system, or on one whose file locks do not work. It has to be on a local disk. |
| `SCHEMA_MISMATCH` | The declared schema differs from the one the file holds at the same version, or the file was migrated since this handle opened it. |
| `SCHEMA_TOO_NEW` | The file holds a newer schema version than the one declared: a newer application wrote it. |
| `DUPLICATE_KEY` | An insert found its primary key taken, or a unique index found a value taken. |
| `INVALID_QUERY` | A query names a field the collection does not have, tests one with a value of another type, or does not parse. |
| `MIGRATION_FAILED` | A migration function reported that it failed. The file keeps its old schema and data. |
| `INTERNAL` | Something only a bug in DaruDB can cause. Please report it. |
| `IO` | The operating system failed an operation on the file. The message says what it reported. |
