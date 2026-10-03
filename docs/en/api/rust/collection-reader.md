---
title: CollectionReader
order: 5
counterpart: /api/node/read-collection
---

# CollectionReader

`CollectionReader` gives the objects of one collection as a read transaction sees them: by primary key, all in key order, or found by a query.

```rust
#[derive(Debug)]
pub struct CollectionReader<'a>
```

[`ReadTransaction::collection`](./read-transaction.md#collection) returns one. It borrows the transaction and lives no longer than it, but the objects it returns are plain [`Object`](../../types/rust/object.md) values that outlive both. Every field of the schema is in an object read back: a field the object was written without holds its default, or null.

A primary key is passed as anything that converts into a [`Value`](../../types/rust/value.md) of the key's type, such as an integer for the auto-increment `id` or a `&str` for a string key. A key of another type fails with `INVALID_ARGUMENT`.

```rust
use darudb::{Database, Filter, Query};

fn read(db: &Database) -> darudb::Result<()> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    if let Some(user) = users.get(1)? {
        println!("{:?}", user.get("name"));
    }

    let adults = Query::new().filter(Filter::ge("age", 18)).sort_by("name");

    for user in users.query(&adults)? {
        println!("{:?}", user.get("name"));
    }

    println!(
        "{} of {} users are adults",
        users.count(&adults)?,
        users.len()?
    );
    Ok(())
}
```

## Methods

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>>
```

The object whose primary key is `key`, or `None`.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_>
```

Every object, in primary key order. An item is an error where the file is damaged.

### len

```rust
pub fn len(&self) -> Result<u64>
```

The number of objects. The count is kept with the collection, so no object is read.

### is_empty

```rust
pub fn is_empty(&self) -> Result<bool>
```

Whether the collection holds no object.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<Object>>
```

The objects [`query`](./query.md) finds, in its order. It fails with `INVALID_QUERY` if the query names a field the collection does not have, tests a field with a value of another type, or is a prepared query run without values for its parameters. [Queries](../../guide/queries.md) explains which queries read an index and which read every object.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

How many objects `query` finds, after its offset and within its limit. A filter that an index or the primary key answers alone is counted without reading the objects. It fails as `query` does.

### get_record

```rust
pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>>
```

The record of the object whose primary key is `key`, as the file holds it, for a language binding that decodes records itself; [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#records) specifies the encoding. It is not checked here, so the binding treats it as untrusted: a record written before a field existed lacks that field, which reads as its default or null, and a record may hold ids of fields the schema no longer has, which are skipped.

### get_record_with

```rust
pub fn get_record_with(
    &self,
    key: impl Into<Value>,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool>
```

Gives `visit` the record that `get_record` returns, borrowed where it lies rather than copied into a vector of its own, and returns whether there was one. A binding that copies the record into a buffer of its own copies it once. An error `visit` returns is returned.

### query_records

```rust
pub fn query_records(&self, query: &Query) -> Result<Vec<Vec<u8>>>
```

The records of the objects `query` finds, in its order, as the file holds them, for a language binding that decodes them itself. An object that the filter and the sort need not read is not decoded at all.

### query_records_with

```rust
pub fn query_records_with(
    &self,
    query: &Query,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()>
```

Gives `visit` the record of each object `query` finds, in its order, borrowed rather than copied into a vector of its own. It stops at the first error `visit` returns, and returns it.
