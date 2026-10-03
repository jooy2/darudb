---
title: Transactions
order: 5
---

# Transactions

Every read happens in a read transaction and every change in a write transaction, which commits whole or not at all.

## Read and write

::: lang rust

`begin_read` starts a read transaction and `begin_write` a write transaction. A write transaction's changes reach the file together when `commit` returns. One dropped without `commit` is aborted, and nothing it did reaches the file.

```rust
use darudb::{Database, Object};

fn add_user(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    txn.collection("users")?.insert(Object::new().with("name", "Alice"))?;
    txn.commit()?;

    let read = db.begin_read()?;
    println!("{} users", read.collection("users")?.len()?);

    Ok(())
}
```

:::

::: lang node

`write` runs a function in a write transaction and commits when the function returns; if it throws, nothing it did is kept. `read` runs a function in a read transaction. Both return what the function returns, and neither lets a transaction outlive its function: a transaction or a collection used after its function has returned throws `CLOSED`.

```ts
const key = db.write((txn) => txn.collection('users').insert({ name: 'Alice' }));

const count = db.read((txn) => txn.collection('users').count());
```

- Transactions are synchronous. A function that returns a promise is refused, and its transaction is aborted. The [asynchronous API](./async.md) takes asynchronous functions.
- Write transactions do not nest: `db.write` inside another's function fails at once, where it would otherwise wait for itself.

:::

A read transaction sees one commit for as long as it lives, whatever is committed after it began, and never waits for a writer. There is one write transaction at a time in a file, across every handle and every process. A write waits for the one already running for up to the busy timeout, five seconds unless <LangCode rust="OpenOptions::busy_timeout" node="busyTimeout" /> says otherwise, and then fails with `BUSY`.

## Sync and deferred commits

A commit waits for the disk before it returns by default. A deferred commit does not: readers see its changes at once, and they reach the disk together with later commits.

::: lang rust

```rust
use darudb::{Database, Object};

fn record_click(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    txn.collection("events")?.insert(Object::new().with("kind", "click"))?;
    txn.commit_deferred()?;

    // Everything committed so far is durable once this returns.
    db.sync()
}
```

:::

::: lang node

```ts
db.write((txn) => txn.collection('events').insert({ kind: 'click' }), { durability: 'deferred' });

// Everything committed so far is durable once this returns.
db.sync();
```

:::

Deferred commits become durable at the next sync commit, at <LangCode rust="Database::sync" node="db.sync()" />, when the database is closed, or once they have waited a second. A crash of the process loses none of them, because they are already in the file. A power cut can undo the newest ones, but it never leaves a gap and never damages the file: what comes back is a commit that was made, with every commit before it.

::: lang rust

`OpenOptions::max_unsynced_time` sets how long a deferred commit may wait, one second by default, and `OpenOptions::max_unsynced_pages` how many pages the deferred commits may have written before the next commit syncs them, 16,384 by default.

:::

A sync commit costs one wait for the disk, which on most machines is the larger part of a small commit. Defer the commits whose loss in a power cut the application can live with, such as a stream of events, and sync the ones it cannot.

## The page cache

Each process keeps the pages it reads in a cache, so that reading a page again costs neither a read nor a check. The cache takes up to 32 MiB for each open file by default, and only as pages are read, so a smaller database never takes all of it. <LangCode rust="OpenOptions::cache_size" node="cacheSize" /> sets the size in bytes: more for a large database that is read often, less in a process with little memory, such as a mobile app extension.

Opening a file that is already open in the process gives another handle to the same database, which shares its cache and its writer.
