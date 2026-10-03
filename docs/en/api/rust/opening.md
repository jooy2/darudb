---
title: Opening and PendingMigration
order: 17
---

# Opening and PendingMigration

`Opening` and `PendingMigration` let a language binding open a database and drive its migration one version step at a time, running a function of its own in each step.

An application does not need them: it opens with [`OpenOptions::open`](./open-options.md#open) and gives its migration functions to [`Migration::run`](./migration.md#run). A binding cannot hand the engine its migration functions as Rust closures, so it opens with `open_migrating` instead, and the migration stops for it between version steps. [Bindings](../../engine/bindings.md) describes how a binding uses them.

```rust
use darudb::{Database, Migrating, OpenOptions, Opening};

/// Opens with `options`, running `step`, the binding's own function, in
/// every version step of a migration.
fn open(
    options: &OpenOptions,
    step: impl Fn(u64, &mut Migrating<'_>) -> darudb::Result<()>,
) -> darudb::Result<Database> {
    let mut pending = match options.open_migrating("app.darudb")? {
        Opening::Open(db) => return Ok(db),
        Opening::Migrating(pending) => pending,
    };

    while let Some(version) = pending.next_step()? {
        step(version, &mut pending.migrating())?;
    }

    pending.finish()
}
```

## open_migrating

```rust
pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening>
```

A method of [`OpenOptions`](./open-options.md). It opens the database like `open`, and fails as `open` does, except that a file holding an older version of the schema returns `Opening::Migrating` with the migration under way rather than running it to the end.

## Opening

```rust
#[derive(Debug)]
pub enum Opening {
    Open(Database),
    Migrating(PendingMigration),
}
```

What opening a database with `open_migrating` leads to.

| Variant     | Description                                                                   |
| ----------- | ----------------------------------------------------------------------------- |
| `Open`      | The database is open, and holds the declared schema                           |
| `Migrating` | The file holds an older version of the schema, and the migration is under way |

### complete

```rust
pub fn complete(self) -> Result<Database>
```

The database, once any migration has run its steps and committed: the database of `Open`, or [`finish`](#finish) of `Migrating`.

## PendingMigration

```rust
#[derive(Debug)]
pub struct PendingMigration
```

A migration under way: the write transaction that migrates the file, and the version steps it has still to run. The stored schema is the declared one already, and new indexes are built. [`next_step`](#next-step) runs the steps in version order; each runs the function its [`Migration`](./migration.md) registered, if any, and returns its version, so that the caller can run a function of its own for the step through [`migrating`](#migrating). [`finish`](#finish) commits the migration, and dropping it instead leaves the file as it was.

It holds the write transaction until then, so other writers in this process and in other processes wait for it, and fail with `BUSY` after the busy timeout.

### previous_version

```rust
pub fn previous_version(&self) -> u64
```

The schema version the file holds.

### version

```rust
pub fn version(&self) -> u64
```

The schema version the migration leads to.

### schema_record

```rust
pub fn schema_record(&self) -> &[u8]
```

The schema the migration leads to, encoded as the file stores it, which the migration's transaction holds already; see [`Database::schema_record`](./database.md#schema-record).

### previous_schema_record

```rust
pub fn previous_schema_record(&self) -> Vec<u8>
```

The schema the file held before the migration, encoded as the file stores it, for a binding that decodes the records of [`Migrating::previous_record`](./migrating.md#previous-record) itself.

### next_step

```rust
pub fn next_step(&mut self) -> Result<Option<u64>>
```

Runs the function of the next version step, if its migration registered one, and returns the step's version; `None` once every step has run. An error ends the migration: drop it, and the file keeps its old schema and data.

### migrating

```rust
pub fn migrating(&mut self) -> Migrating<'_>
```

The migration's write transaction, as a migration function gets it; see [`Migrating`](./migrating.md).

### transaction

```rust
pub fn transaction(&mut self) -> &mut WriteTransaction
```

The migration's write transaction itself, whose collections are those of the new schema. Committing it is `finish`'s job.

### finish

```rust
pub fn finish(self) -> Result<Database>
```

Runs the steps left, deletes the collections the steps delete, and commits the migration. Returns the open database.
