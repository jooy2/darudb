---
title: Field
order: 14
group: objects
counterpart: /types/node/stored-schema
pageClass: reference-page
---

# Field

`Field` is a field of a collection or of an embedded object, as a schema declares it, read back.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Field
```

The builders of [`Collection`](../../api/rust/collection.md) and [`Embedded`](../../api/rust/embedded.md) make fields, and their `fields` methods give them back. A program reads them to look at a schema it did not declare, such as the one [`Database::stored_schema`](../../api/rust/database.md#stored-schema) gives for a file opened without a schema.

```rust
use darudb::{Database, Type};

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;

    for collection in db.stored_schema().iter().flat_map(|schema| schema.collections()) {
        for field in collection.fields() {
            let optional = if field.is_optional() { " (optional)" } else { "" };

            println!("{}.{}: {:?}{optional}", collection.name(), field.name(), field.kind());

            if let Type::Object(embedded) = field.kind() {
                println!("  {} fields of its own", embedded.fields().len());
            }
        }
    }

    Ok(())
}
```

## Methods

### name

```rust
pub fn name(&self) -> &str
```

The field's name.

### kind

```rust
pub fn kind(&self) -> &Type
```

The field's [`Type`](./type.md). An embedded object's fields are in the [`Embedded`](../../api/rust/embedded.md#fields) of its `Type::Object`.

### is_optional

```rust
pub fn is_optional(&self) -> bool
```

Whether the field may be null, which it is when an object leaves it out.

### is_cascade

```rust
pub fn is_cascade(&self) -> bool
```

Whether deleting the object the link names deletes the object that holds it, as [`Collection::cascade`](../../api/rust/collection.md#cascade) declares.

### default

```rust
pub fn default(&self) -> Option<&Value>
```

The [`Value`](./value.md) an object that leaves the field out holds, for a required field that has one.
