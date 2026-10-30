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

### sum

```rust
pub fn sum<V: NumberType>(&self, query: &Query, field: impl Into<PathOf<T, V>>) -> Result<V::Sum>
```

The sum of `field`, a number field of `T` named by its constant or a path from one, over the objects `query` finds, as [`CollectionReader::sum`](./collection-reader.md#sum) works it out: an `i64` for an int field, exact, and an `f64` for a float field. A sum of no values is zero.

### min

```rust
pub fn min<V: OrderedType>(&self, query: &Query, field: impl Into<PathOf<T, V>>) -> Result<Option<V::Compared>>
```

The least value of `field` over the objects `query` finds, in the field's own type, or `None` when there is none: `users.min(&query, User::AGE)` is an `Option<i64>`.

### max

```rust
pub fn max<V: OrderedType>(&self, query: &Query, field: impl Into<PathOf<T, V>>) -> Result<Option<V::Compared>>
```

The greatest value, as `min` gives the least.

### avg

```rust
pub fn avg<V: NumberType>(&self, query: &Query, field: impl Into<PathOf<T, V>>) -> Result<Option<f64>>
```

The mean of `field` over the objects `query` finds, or `None` when there are no values.

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
