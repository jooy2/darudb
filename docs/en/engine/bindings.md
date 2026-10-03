---
title: Building a binding
order: 7
languages: [rust]
---

# Building a binding

This page describes the calls of the Rust crate that a language binding is built on, the byte formats a binding exchanges with the engine, and how the Node.js package divides its work between JavaScript and the engine.

## What a binding does

A binding translates, and the engine decides. Checking objects against the schema, keeping indexes in step, parsing, planning and running queries, changing the stored schema in a migration, and every error code are in the engine, so that every language reads a file the same way. A binding:

- Declares the schema and its migrations in its own language, and hands them to the engine.
- Writes objects as records, and reads records back as objects.
- Builds queries as IR, or hands text in the query language to the engine to parse.
- Runs migration functions in its own language, between the steps of a migration that the engine stops for.
- Passes every error's code through unchanged.

The Node.js package is built this way, with napi-rs, and a Dart package is planned the same way. The formats below are specified in [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md) in the repository.

## Opening with a schema

A binding declares its schema in its own language and encodes it in the format the file stores a schema in, with collection and field ids of its own choosing. [`Schema::decode`](../api/rust/schema.md) reads it. Those ids only tie links and indexes to what they name; the file gives collections and fields ids of its own.

The structural part of each migration, the collections and fields it renames, the collections it deletes and the fields it replaces, is a [`Migration`](../api/rust/migration.md) with no function. The functions stay in the binding's language, and the binding opens the file with `OpenOptions::open_migrating`:

```rust
pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening>
```

It returns [`Opening::Open`](../api/rust/opening.md) with the database when the file holds the declared schema, or once it has stored the schema in a file that held none, and `Opening::Migrating` with a `PendingMigration` when the file holds an older version.

### Running a migration

A [`PendingMigration`](../api/rust/opening.md#pendingmigration) is the migration's write transaction, with the new schema already stored in it and the new indexes built. The binding asks for the version steps one at a time, and runs its own function for each in the same transaction:

```rust
use darudb::{Database, Migrating, Opening, OpenOptions};

/// Runs the binding's own migration function for `version`, if it has one.
fn run_step(version: u64, migrating: &mut Migrating<'_>) -> darudb::Result<()> {
    // Call into the binding's language here.
    let _ = (version, migrating);
    Ok(())
}

fn open(options: &OpenOptions, path: &str) -> darudb::Result<Database> {
    match options.open_migrating(path)? {
        Opening::Open(database) => Ok(database),
        Opening::Migrating(mut pending) => {
            while let Some(version) = pending.next_step()? {
                run_step(version, &mut pending.migrating())?;
            }

            pending.finish()
        }
    }
}
```

| Member of `PendingMigration` | What it gives |
| --- | --- |
| `previous_version`, `version` | The schema version the file holds, and the one the migration leads to |
| `schema_record` | The new schema as the file holds it, with the file's ids |
| `previous_schema_record` | The old schema as the file held it, to decode old records with |
| `next_step` | Runs the next step's Rust function, if its `Migration` has one, and returns the step's version; `None` once every step has run |
| `migrating` | The transaction as a migration function gets it: [`Migrating`](../api/rust/migrating.md), with `collection`, `previous_keys` and `previous_record` |
| `transaction` | The write transaction itself, whose collections are the new schema's |
| `finish` | Runs the steps left, deletes the collections the steps delete, commits, and returns the database |

An error from a step ends the migration. Dropping the `PendingMigration`, whether after an error or instead of finishing, leaves the file with its old schema and data. The migration holds the writer lock until then, so a binding must not let its migration functions wait for a write on the same file.

## Field ids

Records hold field ids rather than names. `Database::schema_record` returns the schema a handle was opened with as the record the file holds it in, with the file's ids for every collection, field and index, or `None` for a handle opened without a schema. The binding decodes it once, and maps names to ids from then on.

```rust
pub fn schema_record(&self) -> Option<&[u8]>
```

## Records

An object crosses the boundary as a record:

```text
record  = count field*          the fields present, ascending by field id
field   = id value
value   = tag payload
```

`count`, `id` and every length are unsigned LEB128 varints, and a field whose value is null is left out.

| Tag    | Type            | Payload                           |
| ------ | --------------- | --------------------------------- |
| `0x02` | `bool` false    | None                              |
| `0x03` | `bool` true     | None                              |
| `0x04` | `int`           | Zigzag LEB128 varint              |
| `0x05` | `float`         | 8 bytes, little-endian            |
| `0x06` | `string`        | Length, then the UTF-8 bytes      |
| `0x07` | `bytes`         | Length, then the bytes            |
| `0x08` | `list`          | Count, then that many values      |
| `0x09` | embedded object | Length, then a record             |
| `0x0A` | `link`          | A value: the target's primary key |

The changes of an update are a record of the fields it changes, in which a field may also hold the tag `0x01` with no payload: the field becomes null. No other record holds that tag.

**Writing.** [`CollectionWriter`](../api/rust/collection-writer.md) takes one record a call:

| Call | What it does |
| --- | --- |
| `insert_record(record)` | Inserts the object, and returns its primary key |
| `put_record(record)` | Inserts the object or replaces the one with its key, and returns its primary key |
| `update_record(key, changes)` | Sets the fields the changes hold, and returns whether the object existed |

Each checks the record against the schema, fills in defaults and the auto-increment key as `insert` does, and keeps the indexes in step. A record that does not decode, or holds an id or a type its collection does not have, is refused with `INVALID_ARGUMENT`, and a primary key or a unique value already taken with `DUPLICATE_KEY`. A refused write leaves the transaction able to commit.

**Reading.** [`CollectionReader`](../api/rust/collection-reader.md), and `CollectionWriter` with the transaction's changes, return records:

| Call | What it does |
| --- | --- |
| `get_record(key)` | The record of the object with that primary key |
| `get_record_with(key, visit)` | The same, lent to `visit` where it lies instead of copied into a vector of its own |
| `query_records(query)` | The records a query finds, in its order |
| `query_records_with(query, visit)` | The same, each lent to `visit` |

The `_with` forms let a binding that copies records into a buffer of its own copy each one once. A record comes from the file, and the engine does not check it on the way out, so the binding treats it as untrusted: a field the record lacks, which only a record written before the field existed can, reads as its default or null, and an id the schema no longer has belongs to a removed field and is skipped.

A primary key crosses as a [`Value`](../types/rust/value.md): `Value::Int`, `Value::String` or `Value::Bytes`, as the collection's key type says. The calls take one record each; batching is the binding's part. The Node.js package sends a batch of objects as one buffer of records, each after its length, and its native layer calls `insert_record` or `put_record` for each, so a batch crosses the language boundary once.

## Queries

A query crosses as IR, in one buffer: a record of a fixed shape.

| Field | Name | Value |
| --- | --- | --- |
| 1 | collection | `string` |
| 2 | filter | An expression: an embedded object whose field 1 is the operator, field 2 the path as a list of strings, field 3 the values, and field 4 the sub-expressions |
| 3 | sort | A list of objects, each a path and whether it is descending |
| 4 | offset | `int` |
| 5 | limit | `int` |
| 6 | count | `bool`: count the objects rather than return them |

A value in an expression may instead be a parameter: an embedded object whose field 1 is the parameter's number.

[`QueryRequest::decode`](../api/rust/query-request.md) reads the IR into the collection, the [`Query`](../api/rust/query.md) and whether to count, and refuses IR that does not decode with `INVALID_QUERY`. The binding then runs the query on the collection:

```rust
use darudb::{QueryRequest, ReadTransaction};

/// What a query in IR finds: the records, or how many there are.
enum Found {
    Records(Vec<Vec<u8>>),
    Count(u64),
}

fn run(txn: &ReadTransaction, ir: &[u8]) -> darudb::Result<Found> {
    let request = QueryRequest::decode(ir)?;
    let collection = txn.collection(&request.collection)?;

    if request.count {
        collection.count(&request.query).map(Found::Count)
    } else {
        collection.query_records(&request.query).map(Found::Records)
    }
}
```

**Text in the query language** is parsed by the engine, so that every binding shares one parser. `Query::prepare` parses it into the same query, keeping `$0`, `$1` and on as parameters, and `Query::bind_encoded` gives them values from one buffer: a record whose field 0 is how many values there are and field `n + 1` the value of parameter `n`, a null one left out. A binding that keeps a prepared query binds it on every run without parsing it again. `QueryRequest::encode` turns a request back into IR, with bound values in their places, for a binding that wants parsed text in its own hands.

The engine checks every query against the collection's schema when it runs, and refuses one that names a field the collection does not have, or compares a field with a value of another type, with `INVALID_QUERY`.

## Errors

Every [`Error`](../types/rust/error.md) has a stable code, `Error::code`, such as `NOT_FOUND` or `DUPLICATE_KEY`. A binding passes it through unchanged, as `error.code` in JavaScript, and uses the same codes for failures of its own, such as `CLOSED` for an object used after it was closed and `INVALID_ARGUMENT` for a value it cannot convert. A code, once released, is never renamed. `Error` is non-exhaustive, so a `match` on it needs a wildcard arm, and matching on the code is often simpler.

## Threads and the event loop

Every engine call blocks its thread until it is done: it may wait for the disk, or for the writer lock up to the busy timeout. A binding for a language with an event loop runs calls elsewhere. The Node.js package's [asynchronous API](../guide/async.md) runs them on the libuv thread pool, through napi-rs `AsyncTask`, with the transaction moved to a pool thread behind a mutex, and sends a transaction's operations in batches, one batch at a time and in the order they were called.

Three rules come with that:

- **Do not let a pool thread wait for the process's own writer.** `begin_write` waits for a write transaction already running in the same process. If tasks waiting for it hold every pool thread while the running one needs a thread to finish, neither moves. The Node.js package queues its process's writes on each file in JavaScript and hands them to the pool one at a time, keyed by the file's device and inode, which on Windows are its volume serial number and file index, the same two the engine reads, so that two paths to one file share a queue. Its `syncAsync` and `closeAsync` queue with them, since a sync waits for the writer.
- **Never open the database file.** On Unix-like systems, closing that descriptor would release every lock the engine holds on the file. Finding a file's device and inode with `stat` opens nothing.
- **End read transactions promptly.** One left open keeps the pages of its commit from reuse in every process, so the file grows. A binding should make a leaked one hard to write, such as by scoping each transaction to a function, as the Node.js package does.

`OpenOptions::key` and `OpenOptions::password` copy the secret into buffers that the engine wipes when it drops them. The Node.js package copies the caller's secret into buffers of its own, builds the options with them before any asynchronous task runs, and then fills its buffers with zeros.

## How the Node.js package divides the work

| In JavaScript, `packages/node/lib` | In the engine |
| --- | --- |
| Declares the schema with `t`, `collection` and `schema`, and encodes it as a schema record (`encodeSchema` in `lib/codec.ts`) | Checks the schema, stores it, compares it with the file's, and gives ids |
| Reads the file's ids from the schema record (`decodeSchema`) | Keeps the stored schema and detects another handle's or process's migration |
| Encodes objects as records and decodes records into objects, with code it generates for each layout | Checks every record against the schema, and writes it with its index entries in step |
| Builds a query's IR with its builder (`encodeQuery`), and the values of its parameters (`encodeParameters`) | Decodes the IR, parses query text, checks the query against the schema, chooses an index and runs the query |
| Runs migration functions between the steps that `next_step` returns | Renames, new fields and indexes, deleting what the steps delete, and the commit |
| Queues each file's writes for the thread pool, and wipes secrets | Locks, transactions, commits, recovery and encryption |

Between the two, the native layer in `packages/node/src/lib.rs` converts arguments, returns the records a query finds in one buffer, each after its length, and turns an `Error` into a JavaScript error with the same `code`.
