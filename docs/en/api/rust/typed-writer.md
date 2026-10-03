---
title: TypedWriter
order: 8
---

# TypedWriter

`TypedWriter` reads and writes the objects of one collection in a write transaction, as the Rust type of the collection.

```rust
pub struct TypedWriter<'a, T>
```

[`WriteTransaction::collection_of`](./write-transaction.md#collection-of) returns one for a type that implements [`CollectionType`](../../types/rust/collection-type.md), which [`#[derive(Object)]`](./derive.md) does. It offers what [`CollectionWriter`](./collection-writer.md) does, with `T` in place of [`Object`](../../types/rust/object.md), and sees the transaction's own changes. It borrows the transaction mutably, so the borrow has to end before the transaction commits.

```rust
use darudb::{Database, Object};

#[derive(Object, Debug, Clone)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(default = 0)]
    age: i64,
}

fn write(db: &Database) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection_of::<User>()?;

    let id = users.insert(&User { id: None, name: "Alice".to_owned(), age: 31 })?;

    if let Some(mut alice) = users.get(id)? {
        alice.age += 1;
        users.put(&alice)?;
    }

    drop(users);
    txn.commit()
}
```

## Methods

### insert

```rust
pub fn insert(&mut self, object: &T) -> Result<T::Key>
```

Inserts `object` and returns its primary key. In a collection keyed by an auto-increment, an object whose `id` is `None` gets the next number, and one with an `id` keeps it. It fails with `DUPLICATE_KEY` if the key is taken, or if a unique index finds one of the object's values taken, and the transaction is left as it was.

### put

```rust
pub fn put(&mut self, object: &T) -> Result<T::Key>
```

Inserts `object`, or replaces the object with its primary key, and returns the key. It fails as `insert` does, except that a taken key is not a failure.

### delete

```rust
pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool>
```

Deletes the object whose primary key is `key`, and returns whether there was one.

### update

```rust
pub fn update(&mut self, key: impl Into<Value>, changes: Object) -> Result<bool>
```

Sets the fields `changes` has in the object whose primary key is `key`, and keeps the rest, as [`CollectionWriter::update`](./collection-writer.md#update) does. The changes are an [`Object`](../../types/rust/object.md) of the fields by name, since a struct would have to hold every field.

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<T>>
```

The object whose primary key is `key`, or `None`, with this transaction's changes.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<T>>
```

The objects `query` finds, in its order, with this transaction's changes.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

How many objects `query` finds, with this transaction's changes.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_>
```

Every object, in primary key order, with this transaction's changes.

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
pub fn untyped(&mut self) -> &mut CollectionWriter<'a>
```

The same collection as a [`CollectionWriter`](./collection-writer.md), for what the typed writer does not offer.
