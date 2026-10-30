---
title: ChangeWhere
order: 8.6
---

# ChangeWhere

`ChangeWhere` holds changes to every object a query finds in a typed collection, one field at a time, and writes them all or nothing.

```rust
pub struct ChangeWhere<'w, 'a, T>
```

[`TypedWriter::change_where`](./typed-writer.md#change-where) makes one for a query. Each field is named by the constant [`#[derive(Object)]`](./derive.md) declares for it, a [`FieldOf`](../../types/rust/field-of.md), and takes a value of the field's own type, as [`Change`](./change.md) takes them for one object. Nothing is written until [`apply`](#apply).

```rust
use darudb::{Database, Filter, Object, Query};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index, default = 0)]
    age: i64,
    active: bool,
}

fn deactivate_minors(db: &Database) -> darudb::Result<u64> {
    let mut txn = db.begin_write()?;
    let minors = Query::new().filter(Filter::lt("age", 18));
    let changed = txn
        .collection_of::<User>()?
        .change_where(&minors)
        .set(User::ACTIVE, false)
        .apply()?;

    txn.commit()?;
    Ok(changed)
}
```

## Methods

### set

```rust
pub fn set<V: FieldType>(self, field: FieldOf<T, V>, value: V) -> Self
```

Sets `field` to `value` in each object, in place of any value set for it before, and returns the changes; see [`Change::set`](./change.md#set).

### apply

```rust
pub fn apply(self) -> Result<u64>
```

Writes the changes to every object the query finds, after its sort, offset and limit, and returns how many there were. All or nothing, as [`CollectionWriter::update_where`](./collection-writer.md#update-where) is: when one object refuses its change, the ones changed before it are put back, and the transaction is left as it was. The changes cannot set the primary key.
