---
title: WriteTransaction
order: 4
---

# WriteTransaction

`WriteTransaction` holds changes to the database that become visible together when it commits, or not at all.

```rust
#[derive(Debug)]
pub struct WriteTransaction
```

[`Database::begin_write`](./database.md#begin-write) starts one. There is one write transaction at a time per file, across every thread and process, and read transactions never wait for it. Its own reads see its changes. [`commit`](#commit) makes the changes durable before it returns, and [`commit_deferred`](#commit-deferred) publishes them without waiting for the disk; [Transactions](../../guide/transactions.md) compares the two. Dropping the transaction without committing aborts it, and nothing it did reaches the file. It is `Send` and `Sync`, so it can be handed to another thread.

If a change to a tree fails, for instance one with a key that is too long, the transaction can no longer commit: `commit` and `commit_deferred` fail with `INVALID_ARGUMENT`, and only dropping it is left. A write to a collection that is refused is the exception. It changes nothing, and the transaction can go on and commit; [`CollectionWriter`](./collection-writer.md) lists those refusals.

The trees and their names follow the rules of [`ReadTransaction`](./read-transaction.md). A key is at most a quarter of a page long, less a few bytes: 957 bytes with the default page size. A value is less than 4 GiB long, and one too large to keep in the tree's pages is stored in pages of its own.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;
    let mut txn = db.begin_write()?;

    txn.insert("users", b"alice", b"admin")?;
    txn.insert("users", b"bob", b"member")?;
    txn.remove("users", b"carol")?;
    txn.commit()?;

    db.close()
}
```

## Methods

### insert

```rust
pub fn insert(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()>
```

Stores `value` under `key` in tree `tree`, replacing any value already there. The tree is created if it does not exist.

### remove

```rust
pub fn remove(&mut self, tree: &str, key: &[u8]) -> Result<bool>
```

Removes `key` and its value from tree `tree`, and returns whether it was there.

### delete_tree

```rust
pub fn delete_tree(&mut self, tree: &str) -> Result<bool>
```

Deletes tree `tree` with everything in it, and returns whether it existed.

### get

```rust
pub fn get(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>>
```

The value stored under `key` in tree `tree`, including changes made in this transaction.

### iter

```rust
pub fn iter(&self, tree: &str) -> Result<Range<'_>>
```

Every entry of tree `tree`, in key order, including changes made in this transaction, as a [`Range`](../../types/rust/range.md). The walk borrows the transaction, so nothing can change while it lives.

### range

```rust
pub fn range<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

The entries of tree `tree` whose keys lie within `range`, in key order, including changes made in this transaction. The range is given as for [`ReadTransaction::range`](./read-transaction.md#range).

### range_backward

```rust
pub fn range_backward<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

The entries of tree `tree` whose keys lie within `range`, in reverse key order, including changes made in this transaction.

### len

```rust
pub fn len(&self, tree: &str) -> Result<u64>
```

The number of entries in tree `tree`, including changes made in this transaction, 0 if it does not exist.

### tree_names

```rust
pub fn tree_names(&self) -> Result<Vec<String>>
```

The names of every tree, including trees created and deleted in this transaction, in byte order. The engine's own trees, which hold the objects of collections, are not among them.

### collection

```rust
pub fn collection(&mut self, name: &str) -> Result<CollectionWriter<'_>>
```

Collection `name` of the schema the handle was opened with, for reading and writing its objects; see [`CollectionWriter`](./collection-writer.md). It borrows the transaction mutably, so one collection is in use at a time, and the borrow has to end before the transaction commits. It fails as [`ReadTransaction::collection`](./read-transaction.md#collection) does.

### collection_of

```rust
pub fn collection_of<T: CollectionType>(&mut self) -> Result<TypedWriter<'_, T>>
```

The collection of `T`, for reading and writing its objects as `T`; see [`TypedWriter`](./typed-writer.md). It borrows the transaction as `collection` does, and fails as [`ReadTransaction::collection_of`](./read-transaction.md#collection-of) does.

### commit

```rust
pub fn commit(mut self) -> Result<()>
```

Makes every change of this transaction visible and durable, together: when it returns, the changes survive a crash or a power cut. If it fails with `SYNC_FAILED`, the outcome is unknown and the database has to be opened again. It fails with `INVALID_ARGUMENT` when an earlier change failed.

### commit_deferred

```rust
pub fn commit_deferred(mut self) -> Result<()>
```

Makes every change of this transaction visible together, without waiting for it to be durable. Readers see the changes as soon as it returns. They become durable at the next barrier: the next `commit`, a call to [`Database::sync`](./database.md#sync), closing the database, or the limits of [`OpenOptions::max_unsynced_pages`](./open-options.md#max-unsynced-pages) and [`max_unsynced_time`](./open-options.md#max-unsynced-time). A deferred commit that would pass those limits is made durable itself.

A crash of the process loses none of the deferred commits. A power cut may undo them, newest first and never leaving a gap, and never damages the file. It fails as `commit` does.

### abort

```rust
pub fn abort(self)
```

Throws away every change of this transaction. It is the same as dropping it.
