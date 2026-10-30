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

Sets the fields `changes` has in the object whose primary key is `key`, and keeps the rest, as [`CollectionWriter::update`](./collection-writer.md#update) does. The changes are an [`Object`](../../types/rust/object.md) of the fields by name, since a struct would have to hold every field. [`set`](#set) and [`change`](#change) name the fields through `T` instead.

### set

```rust
pub fn set<V: FieldType>(&mut self, key: impl Into<Value>, field: FieldOf<T, V>, value: V) -> Result<bool>
```

Sets `field` of the object whose primary key is `key` to `value`, and returns whether there was one; nothing is written when there is none. `field` is the constant [`#[derive(Object)]`](./derive.md#object) declares for the field, a [`FieldOf`](../../types/rust/field-of.md) such as `User::AGE`, and `value` has the field's own type; `None` makes an optional field null.

```rust
users.set(id, User::AGE, 32)?;
```

The value is written as a typed write writes it, and when the field holds a scalar, the record stored is changed where it lies, with any lists, embedded objects and links it holds copied as they are: the object is neither read into `T` nor written whole again, which makes it faster than reading the object and writing it back with [`put`](#put). It fails as `update` does, with `DUPLICATE_KEY` for a value a unique index already holds and `INVALID_ARGUMENT` for a primary key other than `key`, and the transaction is left as it was.

### change

```rust
pub fn change(&mut self, key: impl Into<Value>) -> Change<'_, 'a, T>
```

The changes to the object whose primary key is `key`, made one field at a time with [`Change::set`](./change.md#set) and written together, as one update, by [`Change::apply`](./change.md#apply).

```rust
users.change(id).set(User::NAME, "Alice Liddell".to_owned()).set(User::AGE, 32).apply()?;
```

### delete_where

```rust
pub fn delete_where(&mut self, query: &Query) -> Result<u64>
```

Deletes every object `query` finds, and returns how many there were; see [`CollectionWriter::delete_where`](./collection-writer.md#delete-where).

### update_where

```rust
pub fn update_where(&mut self, query: &Query, changes: Object) -> Result<u64>
```

Sets the fields `changes` has in every object `query` finds, all or nothing; see [`CollectionWriter::update_where`](./collection-writer.md#update-where).

### change_where

```rust
pub fn change_where(&mut self, query: &Query) -> ChangeWhere<'_, 'a, T>
```

The changes to every object `query` finds, made one field at a time with [`ChangeWhere::set`](./change-where.md#set) and written together, all or nothing, by [`ChangeWhere::apply`](./change-where.md#apply).

```rust
users.change_where(&minors).set(User::ACTIVE, false).apply()?;
```

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
