---
title: Change
order: 8.5
---

# Change

`Change` holds changes to one object of a typed collection, one field at a time, and writes them as one update.

```rust
pub struct Change<'w, 'a, T>
```

[`TypedWriter::change`](./typed-writer.md#change) makes one for the object whose primary key it is given. Each field is named by the constant [`#[derive(Object)]`](./derive.md) declares for it, a [`FieldOf`](../../types/rust/field-of.md), and takes a value of the field's own type. Nothing is written until [`apply`](#apply).

```rust
use darudb::{Database, Object};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
}

fn rename(db: &Database, id: i64) -> darudb::Result<bool> {
    let mut txn = db.begin_write()?;
    let updated = txn
        .collection_of::<User>()?
        .change(id)
        .set(User::NAME, "Alice Liddell".to_owned())
        .set(User::EMAIL, None)
        .set(User::AGE, 32)
        .apply()?;

    txn.commit()?;
    Ok(updated)
}
```

## Methods

### set

```rust
pub fn set<V: FieldType>(self, field: FieldOf<T, V>, value: V) -> Self
```

Sets `field` to `value`, in place of any value set for it before, and returns the changes. `None` makes an optional field null. A value that cannot be written is kept as the error [`apply`](#apply) returns.

### apply

```rust
pub fn apply(self) -> Result<bool>
```

Writes the changes to the object, and returns whether there was one; nothing is written when there is none. It fails as [`TypedWriter::set`](./typed-writer.md#set) does, leaving the transaction as it was.
