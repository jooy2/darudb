---
title: ReadTransaction
order: 3
---

# ReadTransaction

`ReadTransaction` is a consistent view of the database as of one commit, for reading the storage kernel's trees and the collections of a schema.

```rust
#[derive(Debug)]
pub struct ReadTransaction
```

[`Database::begin_read`](./database.md#begin-read) starts one. Everything read through it comes from the commit it began at, whatever is committed while it lives, and it ends when it is dropped; there is nothing to commit. Pages it can reach are not reused until then, so a read transaction kept open for a long time makes the file grow while others write. It is `Send` and `Sync`.

The storage kernel stores named trees of byte keys and byte values, with keys ordered as unsigned bytes; [The storage kernel](../../engine/storage-kernel.md) describes them. A tree name is 1 byte or more, no longer than the longest key the page size allows (957 bytes with 4096-byte pages), and an empty or longer name fails with `INVALID_ARGUMENT`. Names that begin with a NUL character belong to the engine's own trees, which hold the objects of collections, and fail the same way. Reading a tree that does not exist finds nothing. Collections are read through [`collection`](#collection) instead.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;
    let read = db.begin_read()?;

    if let Some(role) = read.get("users", b"alice")? {
        println!("alice is {}", String::from_utf8_lossy(&role));
    }

    for entry in read.range("users", b"a".as_slice()..b"c".as_slice())? {
        let (key, value) = entry?;

        println!(
            "{} = {}",
            String::from_utf8_lossy(&key),
            String::from_utf8_lossy(&value)
        );
    }

    Ok(())
}
```

## Methods

### commit_id

```rust
pub fn commit_id(&self) -> u64
```

The transaction id of the commit this transaction sees. It only grows from one commit to the next, so comparing it with an earlier value tells whether anything was committed in between.

### get

```rust
pub fn get(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>>
```

The value stored under `key` in tree `tree`, or `None`. A tree that does not exist holds nothing.

### iter

```rust
pub fn iter(&self, tree: &str) -> Result<Range<'_>>
```

Every entry of tree `tree`, in key order, as a [`Range`](../../types/rust/range.md) of keys and values.

### range

```rust
pub fn range<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

The entries of tree `tree` whose keys lie within `range`, in key order. Any range of byte strings will do: `b"a".as_slice()..b"c".as_slice()` walks the keys from `a` up to, but not including, `c`, and `key..` every key from `key` on.

### range_backward

```rust
pub fn range_backward<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

The entries of tree `tree` whose keys lie within `range`, in reverse key order: the last key first. In a tree whose keys grow, it reads the newest entries first.

### len

```rust
pub fn len(&self, tree: &str) -> Result<u64>
```

The number of entries in tree `tree`, 0 if it does not exist. The count is kept with the tree, so no entry is read.

### tree_names

```rust
pub fn tree_names(&self) -> Result<Vec<String>>
```

The names of every tree, in byte order. The engine's own trees, which hold the objects of collections, are not among them.

### collection

```rust
pub fn collection(&self, name: &str) -> Result<CollectionReader<'_>>
```

Collection `name` of the schema the handle was opened with, for reading its objects; see [`CollectionReader`](./collection-reader.md). It borrows the transaction, and several collections can be read at once.

It fails with `INVALID_ARGUMENT` if the schema has no such collection or the database was opened without a schema, and with `SCHEMA_MISMATCH` if the commit this transaction sees holds another schema, because another handle or process has migrated the file since this handle opened it.

### collection_of

```rust
pub fn collection_of<T: CollectionType>(&self) -> Result<TypedReader<'_, T>>
```

The collection of `T`, for reading its objects as `T`: the collection named `T::COLLECTION`, whose fields have to be `T`'s, with the same types; see [`TypedReader`](./typed-reader.md) and [Derive macros](./derive.md). It fails as `collection` does, and with `INVALID_ARGUMENT`, naming the field, if the stored collection does not match `T`. The match is worked out once for each handle and type.
