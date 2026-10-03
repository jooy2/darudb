---
title: Object
order: 1
counterpart: /types/node/object-types
---

# Object

`Object` is a set of field values by name, which is what a collection stores and what reading a collection returns.

```rust
#[derive(Clone, PartialEq, Default)]
pub struct Object
```

A program builds one with `Object::new` and `with` to pass to [`insert`, `put` or `update`](../../api/rust/collection-writer.md), and gets one back from [`get`](../../api/rust/collection-reader.md), `iter` and queries. Each value is a [`Value`](./value.md).

An object read from a database is a copy. It holds every field of its collection's schema, the auto-increment `id` included, with [`Value::Null`](./value.md) for an optional field that has no value, and it stays valid after the transaction and the database are gone. An object a program builds holds only the fields set on it, and the write fills in the rest from the schema: [Value](./value.md#field-types) has the rules.

The fields are kept sorted by name in byte order, so `fields` gives them in that order whatever order they were set in, and two objects with the same fields and values are equal however they were built. `Debug` prints an object as a map, such as `{"age": Int(31), "name": String("Alice")}`.

```rust
use darudb::{Database, Object, Value};

fn add_user(db: &Database) -> darudb::Result<Value> {
    let mut txn = db.begin_write()?;
    let id = txn
        .collection("users")?
        .insert(Object::new().with("name", "Alice").with("email", "alice@example.com"))?;

    txn.commit()?;
    Ok(id)
}

fn email_of(db: &Database, id: i64) -> darudb::Result<Option<String>> {
    let read = db.begin_read()?;
    let user = read.collection("users")?.get(id)?;

    Ok(user.and_then(|user| user.get("email")?.as_str().map(str::to_owned)))
}
```

## Associated functions

### new

```rust
pub fn new() -> Self
```

An object with no field set. `Object::default()` is the same.

## Methods

### with

```rust
pub fn with(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self
```

Returns this object with field `name` set to `value`, replacing any value it had. It takes anything that converts into a `Value`, such as `"text"`, `42` or `None::<i64>`; [Value](./value.md#conversions) lists the conversions.

### set

```rust
pub fn set(&mut self, name: impl Into<String>, value: impl Into<Value>) -> Option<Value>
```

Sets field `name` to `value` in place, and returns the value it replaced, or `None` if the object did not have the field.

### get

```rust
pub fn get(&self, name: &str) -> Option<&Value>
```

The value of field `name`, or `None` if the object does not have the field. In an object read from a database every field of the schema is present, so `None` means the name is not a field, and an optional field without a value gives `Some(&Value::Null)`.

### remove

```rust
pub fn remove(&mut self, name: &str) -> Option<Value>
```

Takes field `name` out of the object and returns its value, or `None` if the object did not have it.

### fields

```rust
pub fn fields(&self) -> impl Iterator<Item = (&str, &Value)>
```

The fields with their values, by name in byte order.

### len

```rust
pub fn len(&self) -> usize
```

The number of fields the object has.

### is_empty

```rust
pub fn is_empty(&self) -> bool
```

Whether the object has no field.
