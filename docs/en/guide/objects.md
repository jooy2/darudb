---
title: Collections and objects
order: 3
---

# Collections and objects

A database opened with a schema holds collections of typed objects, with indexes the engine keeps in step with them.

## Declare a schema

A schema has a version, from 1 up, and collections. Each collection has fields of a type, a primary key, and any indexes.

::: lang rust

```rust
use darudb::{Collection, OpenOptions, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;
    db.close()
}
```

The types are `Bool`, `Int` (64-bit), `Float` (64-bit), `String`, `Bytes`, a link to another collection's object (`Type::link`), a list of any of those (`Type::list`), and an embedded object with fields of its own (`Type::object(Embedded::new().field(...))`).

- **Required and optional fields.** `field` is required, and writing an object without it fails. `optional` may be null, which is what it holds when it is left out. `with_default` is required, and an object written without it gets the default.
- **Primary keys.** `primary_key` names a field of type `Int`, `String` or `Bytes`.
- **Indexes.** `index` keeps an index on a field, and `unique` an index that also refuses two objects with the same value.

:::

::: lang node

`t` has the field types, `collection` groups fields, and `schema` gives the collections a version. The TypeScript type of every object follows from the declaration.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  teams: collection({
    name: t.string().primaryKey(),
    city: t.string().optional()
  }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index(),
    tags: t.list(t.string()).optional().index(),
    team: t.link('teams').optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
  })
});

const db = Database.open('app.darudb', { schema: app });
```

The types are `t.bool()`, `t.int()`, `t.bigint()`, `t.float()`, `t.string()`, `t.bytes()`, `t.link(collection)`, `t.list(type)` and `t.object(fields)`.

- **Required and optional fields.** A field is required unless it says otherwise, and writing an object without it fails. `optional()` lets it be null, which is what it holds when it is left out. `default(value)` keeps it required and fills it in when it is left out.
- **Primary keys.** `primaryKey()` makes an `int`, `bigint`, `string` or `bytes` field the key.
- **Indexes.** `index()` keeps an index on a field, and `unique()` an index that also refuses two objects with the same value.
- **Numbers.** A `t.int()` field holds a number. A value beyond 2^53, which a number does not hold exactly, is refused when written and fails when read; declare such a field with `t.bigint()`, which always reads as a `bigint`. Bytes are a `Uint8Array`.

:::

The rules the engine keeps are the same in every language:

- **The automatic key.** A collection that names no primary key gets an integer field called `id`, and an object written without an `id` gets the next number, from 1 up. A number is never given twice in one file, even after its object is deleted.
- **Links** hold the primary key of an object in the linked collection. A link to an object that does not exist is allowed, and reads as the key it holds.
- **Indexes** let a query on the field read only the objects it finds. Any number of objects can hold null in a unique field, and an index on a list has an entry for each element.

The first open stores the schema in the file. Every later open compares the declared schema with the stored one: the same version with a different schema fails with `SCHEMA_MISMATCH`, and a file holding a newer version fails with `SCHEMA_TOO_NEW`. Declaring collections or indexes in another order is not a change. To change the schema, raise its version: see [Migrations](./migrations.md).

## Read and write objects

::: lang rust

An object is a set of named values. Inside a write transaction, `collection` gives a collection's objects and the calls that change them.

```rust
use darudb::{Database, Object, Value};

fn write(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;

    let alice = users.insert(Object::new().with("name", "Alice").with("email", "alice@example.com"))?;
    users.insert(Object::new().with("name", "Bob"))?;

    // `put` replaces the object with the same key.
    users.put(Object::new().with("id", alice.clone()).with("name", "Alice").with("age", 31))?;
    // `update` sets the fields it is given and keeps the rest.
    users.update(alice.clone(), Object::new().with("age", 32).with("email", Value::Null))?;

    let mut posts = txn.collection("posts")?;
    posts.insert(
        Object::new()
            .with("slug", "hello")
            .with("author", alice)
            .with("tags", vec![Value::from("intro")]),
    )?;

    txn.commit()
}

fn read(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    if let Some(user) = users.get(1)? {
        println!("{:?}", user.get("name"));
    }

    for user in users.iter()? {
        println!("{:?}", user?);
    }

    println!("{} users", users.len()?);
    Ok(())
}
```

- `insert` returns the new object's key. `put` inserts or replaces. `delete` takes a key and returns whether there was an object.
- `update` takes a key and the fields to change, and returns whether there was an object; it inserts nothing when there is none. Null makes an optional field null and gives a field with a default its default.

:::

::: lang node

`write` runs a function in a write transaction, and `read` in a read transaction. Inside, `collection` gives a collection's objects and the calls that change them.

```ts
db.write((txn) => {
  txn.collection('teams').insert({ name: 'north', city: 'Seoul' });

  const users = txn.collection('users');

  users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31, team: 'north' },
    { name: 'Bob', tags: ['new'] }
  ]);
  users.put({ id: 2, name: 'Robert', age: 18 });
  users.update(1, { age: 32, email: null });
  users.delete(3);
});

const alice = db.read((txn) => txn.collection('users').get(1));
```

- `insert` and `insertMany` return the keys. `put` and `putMany` insert or replace. `delete` says whether there was an object.
- `update` sets the fields it is given, keeps the rest, and says whether there was an object. `null` makes an optional field null and gives a field with a default its default, and a field left `undefined` stays as it is.
- A batch crosses into the engine as one buffer in one call, which is much cheaper than one call per object.

:::

These hold in every language:

- `insert` fails with `DUPLICATE_KEY` if the key is taken, or if a unique index finds one of the object's values taken.
- An `update` replaces an embedded object or a list whole, and changing the primary key fails with `INVALID_ARGUMENT`. It costs less than reading the object and putting it back, since the engine changes the record where it lies.
- An object that does not fit the schema, with a value of the wrong type, a field the schema does not have, or a required field missing, fails with `INVALID_ARGUMENT`.
- A refused write changes nothing, and the transaction can go on and commit.
- Objects read back are plain values that outlive the transaction. Every field of the schema is there: a left-out field holds its default, or null.

## Several handles and processes

Each handle keeps the schema it was opened with. When another process, or another handle in the same process, migrates the file, the next transaction to reach a collection through the old handle fails with `SCHEMA_MISMATCH`, and the handle has to be opened again with the new schema. A read transaction that began before the migration goes on reading under the old schema, since it sees the commit it began at.
