---
title: CollectionWriter
order: 6
counterpart: /api/node/write-collection
---

# CollectionWriter

`CollectionWriter` gives the objects of one collection inside a write transaction, with the transaction's changes, and the calls that insert, replace, update and delete them.

```rust
#[derive(Debug)]
pub struct CollectionWriter<'a>
```

[`WriteTransaction::collection`](./write-transaction.md#collection) returns one. It borrows the transaction mutably, so one collection is in use at a time, and the borrow has to end before the transaction commits; asking the transaction for another collection ends the use of the one before. Its reads see the transaction's changes and work as those of [`CollectionReader`](./collection-reader.md) do, keys included.

A write that is refused changes nothing, and the transaction can go on and commit:

- **`DUPLICATE_KEY`**: an insert finds its primary key taken, or a unique index finds one of the object's values taken by another object.
- **`INVALID_ARGUMENT`**: the object does not fit the schema. A name that is not a field, a value of the wrong type, a required field left out with no default, a key or an indexed value too long for the file's keys, or a record of 4 GiB or more.

[Collections and objects](../../guide/objects.md) shows the calls together.

```rust
use darudb::{Database, Object, Value};

fn write(db: &Database) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;

    let alice = users.insert(
        Object::new()
            .with("name", "Alice")
            .with("email", "alice@example.com"),
    )?;

    users.update(
        alice.clone(),
        Object::new().with("age", 32).with("email", Value::Null),
    )?;
    users.put(
        Object::new()
            .with("id", alice)
            .with("name", "Alice")
            .with("age", 33),
    )?;
    users.delete(7)?;

    txn.commit()
}
```

## Methods

### insert

```rust
pub fn insert(&mut self, object: Object) -> Result<Value>
```

Inserts `object` and returns its primary key. In a collection keyed by an auto-increment, an object without an `id`, or with a null one, gets the next number, from 1 up. A number is never given twice in one file, even after its object is deleted, and an object that brings its own `id` keeps it, with later numbers starting above it.

### put

```rust
pub fn put(&mut self, object: Object) -> Result<Value>
```

Inserts `object`, or replaces the object with its primary key, and returns the key. It fails as `insert` does, except that a taken key is not a failure. A refused replacement leaves the object it would have replaced as it was.

### update

```rust
pub fn update(&mut self, key: impl Into<Value>, changes: Object) -> Result<bool>
```

Sets the fields `changes` has in the object whose primary key is `key`, and returns whether there was one; nothing is written when there is none. The object becomes what `put` would write for the stored object with those fields set: a field set to `Value::Null` becomes null, or its default if it is required and has one, a required field without a default cannot be made null, and an embedded object or a list is replaced whole. It costs less than reading the object and putting it back.

It fails as `put` does, and with `INVALID_ARGUMENT` when `changes` holds a primary key other than `key`.

### delete

```rust
pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool>
```

Deletes the object whose primary key is `key` with its index entries, and returns whether there was one.

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>>
```

The object whose primary key is `key`, or `None`, with this transaction's changes.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_>
```

Every object, in primary key order, with this transaction's changes.

### len

```rust
pub fn len(&self) -> Result<u64>
```

The number of objects, with this transaction's changes.

### is_empty

```rust
pub fn is_empty(&self) -> Result<bool>
```

Whether the collection holds no object.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<Object>>
```

The objects [`query`](./query.md) finds, in its order, with this transaction's changes. It fails as [`CollectionReader::query`](./collection-reader.md#query) does.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

How many objects `query` finds, with this transaction's changes; see [`CollectionReader::count`](./collection-reader.md#count).

### insert_record

```rust
pub fn insert_record(&mut self, record: &[u8]) -> Result<Value>
```

Inserts the object whose record is `record`, as a language binding sends it: the fields it has, by id, which the write checks and fills in as `insert` does. A record that does not decode, or holds an id or a type its collection does not have, fails with `INVALID_ARGUMENT`. [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#records) specifies records.

### put_record

```rust
pub fn put_record(&mut self, record: &[u8]) -> Result<Value>
```

Inserts or replaces the object whose record is `record`, as `put` does for an object; see `insert_record`.

### update_record

```rust
pub fn update_record(&mut self, key: impl Into<Value>, changes: &[u8]) -> Result<bool>
```

Sets the fields of the object whose primary key is `key` that the record `changes` holds, as a language binding sends them: by id, a field to be made null holding the tag `0x01`. Otherwise it is `update`. In a collection whose fields all hold scalars, the stored record is changed where it lies, and only the indexes on the fields changed are read.

### get_record

```rust
pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>>
```

The record of the object whose primary key is `key`, with this transaction's changes, for a language binding; see [`CollectionReader::get_record`](./collection-reader.md#get-record).

### get_record_with

```rust
pub fn get_record_with(
    &self,
    key: impl Into<Value>,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool>
```

Gives `visit` the record of the object whose primary key is `key`, with this transaction's changes, borrowed rather than copied; see [`CollectionReader::get_record_with`](./collection-reader.md#get-record-with).

### query_records

```rust
pub fn query_records(&self, query: &Query) -> Result<Vec<Vec<u8>>>
```

The records of the objects `query` finds, with this transaction's changes; see [`CollectionReader::query_records`](./collection-reader.md#query-records).

### query_records_with

```rust
pub fn query_records_with(
    &self,
    query: &Query,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()>
```

Gives `visit` the record of each object `query` finds, with this transaction's changes; see [`CollectionReader::query_records_with`](./collection-reader.md#query-records-with).
