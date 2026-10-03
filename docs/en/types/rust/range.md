---
title: Range
order: 9
---

# Range

`Range` is the iterator over a tree's entries that `range`, `range_backward` and `iter` of a transaction return.

```rust
#[derive(Debug)]
pub struct Range<'a>
```

[`ReadTransaction`](../../api/rust/read-transaction.md) and [`WriteTransaction`](../../api/rust/write-transaction.md) both return it. It walks one of the storage kernel's named trees of byte keys and byte values: `range` and `iter` in key order, and `range_backward` in reverse key order, the last key first. Keys are ordered as unsigned bytes. [The storage kernel](../../engine/storage-kernel.md) describes the trees.

The walk borrows the transaction, so it cannot outlive it. A read transaction's walk sees the transaction's snapshot whatever is committed meanwhile, and a write transaction's walk sees the changes the transaction has made so far; while the walk lives, the borrow keeps the write transaction from changing anything.

## Items

```rust
impl Iterator for Range<'_> {
    type Item = Result<(Vec<u8>, Vec<u8>)>;
}
```

Each item is a key and its value, copied out of the page. A tree that does not exist is walked as an empty one.

The bounds are any Rust range over byte strings: `b"a".as_slice()..b"c".as_slice()` walks the keys from `a` up to, but not including, `c`, and `key..` every key from `key` on. A range with no bounds needs its key type named, as in `range::<&[u8]>("users", ..)`, or `iter` instead.

```rust
use darudb::Database;

fn newest(db: &Database, count: usize) -> darudb::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let read = db.begin_read()?;

    read.range_backward::<&[u8]>("events", ..)?.take(count).collect()
}
```

## Errors

The call that returns the walk fails with `INVALID_ARGUMENT` for a tree name that is empty, longer than the longest key the page size allows, or starts with a NUL character, which only the engine's own trees do. A write transaction's call fails with `SYNC_FAILED` after a barrier failed on the file.

The call reads the pages down to where the walk starts, and each step reads more, so both can fail with `CORRUPTED`, when a page does not match the check its parent recorded, or with `IO`, when reading fails. An item that is an error ends the walk: the next call to `next` returns `None`, since a damaged page cannot be stepped over.
