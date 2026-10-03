---
title: TypedReader
order: 7
---

# TypedReader

`TypedReader` gives the objects of one collection as a read transaction sees them, read straight into the Rust type of the collection.

```rust
pub struct TypedReader<'a, T>
```

[`ReadTransaction::collection_of`](./read-transaction.md#collection-of) returns one for a type that implements [`CollectionType`](../../types/rust/collection-type.md), which [`#[derive(Object)]`](./derive.md) does. It offers what [`CollectionReader`](./collection-reader.md) does, with `T` in place of [`Object`](../../types/rust/object.md). A record is decoded into `T` where it lies, so a typed read skips the vector of named values an `Object` is built from.

```rust
use darudb::{Database, Filter, Object, Query};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index)]
    age: i64,
}

fn read(db: &Database) -> darudb::Result<()> {
    let read = db.begin_read()?;
    let users = read.collection_of::<User>()?;

    if let Some(user) = users.get(1)? {
        println!("{}", user.name);
    }

    for user in users.query(&Query::new().filter(Filter::ge("age", 18)).sort_by("name"))? {
        println!("{} is {}", user.name, user.age);
    }

    Ok(())
}
```

## Methods

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<T>>
```

The object whose primary key is `key`, or `None`. A key of another type than the collection's fails with `INVALID_ARGUMENT`.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<T>>
```

The objects [`query`](./query.md) finds, in its order. It fails as [`CollectionReader::query`](./collection-reader.md#query) does.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

How many objects `query` finds, after its offset and within its limit, as [`CollectionReader::count`](./collection-reader.md#count) counts them.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_>
```

Every object, in primary key order. An item is an error where the file is damaged.

### len

```rust
pub fn len(&self) -> Result<u64>
```

The number of objects.

### is_empty

```rust
pub fn is_empty(&self) -> Result<bool>
```

Whether the collection holds no object.

### untyped

```rust
pub fn untyped(&self) -> &CollectionReader<'a>
```

The same collection as a [`CollectionReader`](./collection-reader.md), for what the typed reader does not offer, such as records for a language binding.
