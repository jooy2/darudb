---
title: Derive macros
order: 12
counterpart: /api/node/schema
---

# Derive macros

`#[derive(Object)]` makes a struct the objects of a collection, and `#[derive(Embedded)]` makes one an embedded object. A schema declares the collection from the struct, and a transaction reads records straight into it and writes it as records.

```toml
[dependencies]
darudb = { version = "1.0", features = ["derive"] }
```

The macros come with the crate's `derive` feature, which is off by default so that a program that does not use them does not build a Rust parser. Without it, the [`CollectionType`](../../types/rust/collection-type.md) and [`FieldType`](../../types/rust/field-type.md) traits can be implemented by hand.

```rust
use darudb::{Collection, Embedded, Filter, Link, Object, OpenOptions, Query, Schema};

#[derive(Embedded, Debug, Clone, PartialEq)]
struct Address {
    city: String,
    #[darudb(rename = "zip")]
    postal_code: Option<String>,
}

#[derive(Object, Debug, Clone, PartialEq)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
    tags: Vec<String>,
    address: Option<Address>,
}

#[derive(Object, Debug, Clone, PartialEq)]
#[darudb(collection = "posts")]
struct Post {
    #[darudb(key)]
    slug: String,
    #[darudb(index)]
    author: Link<User>,
}

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1)
        .collection(Collection::of::<User>())
        .collection(Collection::of::<Post>());
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let alice = txn.collection_of::<User>()?.insert(&User {
        id: None,
        name: "Alice".to_owned(),
        email: Some("alice@example.com".to_owned()),
        age: 31,
        tags: vec!["admin".to_owned()],
        address: Some(Address { city: "Seoul".to_owned(), postal_code: None }),
    })?;
    txn.collection_of::<Post>()?.insert(&Post {
        slug: "hello".to_owned(),
        author: Link::new(alice),
    })?;
    txn.commit()?;

    let read = db.begin_read()?;
    let adults: Vec<User> = read
        .collection_of::<User>()?
        .query(&Query::new().filter(Filter::ge("age", 18)))?;

    println!("{adults:?}");
    Ok(())
}
```

## Object

```rust
#[proc_macro_derive(Object, attributes(darudb))]
```

Implements [`CollectionType`](../../types/rust/collection-type.md) for a struct with named fields and no generic parameters. [`Collection::of`](./collection.md#of) declares its collection, and [`ReadTransaction::collection_of`](./read-transaction.md#collection-of) and [`WriteTransaction::collection_of`](./write-transaction.md#collection-of) read and write its objects.

- **The collection** is named after the struct, as written, unless `#[darudb(collection = "name")]` on the struct names it.
- **The primary key** is the field marked `#[darudb(key)]`, of type `i64`, `String` or `Vec<u8>`. Without one, the collection is keyed by an auto-increment, and the struct needs a field `id: Option<i64>`: `None` until the object is inserted, which assigns the next number.
- **The fields** are declared in the struct's order, each with the type its Rust type gives: see [`FieldType`](../../types/rust/field-type.md). An `Option` is an optional field, and anything else is required.

## Embedded

```rust
#[proc_macro_derive(Embedded, attributes(darudb))]
```

Implements [`EmbeddedType`](../../types/rust/collection-type.md#embeddedtype) and [`FieldType`](../../types/rust/field-type.md) for a struct with named fields, so that a field of an object, or of another embedded object, can hold it. [`Embedded::of`](./embedded.md#of) declares its fields. An embedded object has no key, so its fields take neither `key` nor an index.

## Field attributes

| Attribute | Meaning |
| --- | --- |
| `#[darudb(key)]` | The field is the primary key. `Object` only. |
| `#[darudb(index)]` | An index on the field. `Object` only. |
| `#[darudb(unique)]` | An index that also refuses two objects with the same value. `Object` only. |
| `#[darudb(rename = "x")]` | The field's name in the collection, which is the Rust field's name otherwise. |
| `#[darudb(default = 18)]` | The value a record that leaves the field out holds. The field is required. Not on the key. |

A default is anything [`Value::from`](../../types/rust/value.md) takes. Attributes combine in one list, `#[darudb(index, default = 0)]`.

## What reading and writing check

The struct and the stored collection are compared by name once for each handle and type, the first time a transaction reaches the collection as that type. They have to hold the same fields, with the same types and the same optional fields, and the same primary key; otherwise `collection_of` fails with `INVALID_ARGUMENT`, naming the field. A struct that declared the schema matches it, and so does any struct with the same fields in another order.

A typed write is checked by the engine as any write is: a value a unique index already holds fails with `DUPLICATE_KEY`, and the transaction goes on as it was. A record that does not read as the struct, which only a damaged file holds, fails with `CORRUPTED`.
