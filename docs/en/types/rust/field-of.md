---
title: FieldOf
order: 6.5
group: objects
pageClass: reference-page
---

# FieldOf

`FieldOf<T, V>` names a field of the collection type `T` whose values are of type `V`, so that an update names the field through the type rather than by a name.

```rust
pub struct FieldOf<T, V>
```

[`#[derive(Object)]`](../../api/rust/derive.md#object) declares one for each field of the struct, as an associated constant named after the field in capitals, with the field's visibility: `User::AGE` for a field `age`, of type `FieldOf<User, i64>`. [`TypedWriter::set`](../../api/rust/typed-writer.md#set) and [`Change::set`](../../api/rust/change.md#set) take it with a value of type `V`, so that a value of another type does not compile.

```rust
use darudb::{Database, Object};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index, default = 0)]
    age: i64,
}

fn birthday(db: &Database, id: i64, age: i64) -> darudb::Result<bool> {
    let mut txn = db.begin_write()?;
    let updated = txn.collection_of::<User>()?.set(id, User::AGE, age + 1)?;

    txn.commit()?;
    Ok(updated)
}
```

A constant is a number, the field's place among the struct's fields as [`CollectionType::write_field`](./collection-type.md) counts them: the auto-increment `id` first, when the collection has one. A hand-written `CollectionType` makes its own with `FieldOf::new`.

## Methods

### new

```rust
pub const fn new(slot: usize) -> Self
```

The field numbered `slot` in `T`.

### slot

```rust
pub const fn slot(self) -> usize
```

The field's number in `T`.
