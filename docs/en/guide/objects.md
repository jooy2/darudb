---
title: Collections and objects
order: 3
---

# Collections and objects

A database opened with a schema holds collections of typed objects, with indexes the engine keeps in step and migrations from one schema version to the next. This page shows the Rust API; [Node.js](./nodejs.md) has the same in JavaScript.

## Declare a schema

A schema has a version, from 1 up, and collections. Each collection has fields of a type, a primary key, and any indexes.

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

The types are `Bool`, `Int` (64-bit), `Float` (64-bit), `String`, `Bytes`, a link to another collection's object, a list of any of those, and an embedded object with fields of its own (`Type::object(Embedded::new().field(...))`).

- **Required and optional fields.** `field` is required, and writing an object without it fails. `optional` may be null, which is what it holds when it is left out. `with_default` is required, and an object written without it gets the default.
- **Primary keys.** `primary_key` names a field of type `Int`, `String` or `Bytes`. Without one, the collection gets an `Int` field called `id`, and an object written without an `id` gets the next number, from 1 up. A number is never given twice in one file, even after its object is deleted.
- **Links** hold the primary key of an object in the linked collection. A link to an object that does not exist is allowed.
- **Indexes.** `index` keeps an index on a field so that queries on it will not have to read every object, and `unique` also refuses two objects with the same value. Any number of objects can hold null in a unique field. An index on a list has an entry for each element.

The first open stores the schema in the file. Every later open compares the declared schema with the stored one: the same version with a different schema fails with `SCHEMA_MISMATCH`, and a file holding a newer version fails with `SCHEMA_TOO_NEW`. Declaring collections or indexes in another order is not a change.

## Read and write objects

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

- `insert` fails with `DUPLICATE_KEY` if the key is taken, or if a unique index finds one of the object's values taken. `put` inserts or replaces. `delete` takes a key and returns whether there was an object.
- `update` takes a key and the fields to change, and returns whether there was an object; it inserts nothing when there is none. The object becomes what `put` would write for it with those fields set: null makes an optional field null and gives a field with a default its default, and an embedded object or a list is replaced whole. Changing the primary key fails with `INVALID_ARGUMENT`. It costs less than reading the object and putting it back, since the engine changes the record where it lies.
- An object that does not fit the schema, with a value of the wrong type or without a required field, fails with `INVALID_ARGUMENT`.
- A refused write changes nothing, and the transaction can go on and commit.
- Objects read back are plain values that outlive the transaction. Every field of the schema is there: a left-out field holds its default, or null.

## Query objects

A `Query` says which objects to find, in what order, and how many. `query` returns them, and `count` counts them.

```rust
use darudb::{Database, Filter, Query};

fn adults(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);

    for user in users.query(&query)? {
        println!("{:?}", user.get("name"));
    }

    let adults = users.count(&Query::new().filter(Filter::ge("age", 18)))?;
    println!("{adults} adults");
    Ok(())
}
```

- **Conditions** are `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `between`, `is_in`, `contains`, `starts_with`, `ends_with`, `is_null` and `is_not_null`, combined with `and`, `or` and `!`.
- **A path** names a field, or goes through an embedded object or a link with `.`: `address.city`, or `author.name` to test the linked object. A link to an object that is not there reads as null.
- **Lists.** A condition on a list holds when it holds for any element, and `contains` on a list looks for an element. An empty list is not null.
- **Null.** Every condition on a null field is false, except `is_null`. `Filter::eq(field, Value::Null)` is `is_null`.
- **Types.** A value has the field's type: an `Int` field compares with an int, never a float, a `Float` field with any number, and a link with the linked collection's key. A query that breaks this, or names a field that is not there, fails with `INVALID_QUERY`.
- **Order.** Without a sort, objects come in primary key order, and objects that sort equal come in primary key order too. Null sorts first ascending and last descending. Strings compare by their bytes.

The same query can be written as text, which is convenient for queries that do not change and is how the other languages will write them too:

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let query = Query::parse(
        r#"age >= $0 AND name STARTSWITH "A" SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;
    let _ = query;
    Ok(())
}
```

A filter comes first, then `SORT BY`, `LIMIT` and `OFFSET`, each optional. Keywords are case-insensitive, strings are in double quotes, and a field named like a keyword, such as `limit`, goes in backticks. `$0`, `$1` and on take the values passed with the text. A value that comes from outside the program belongs in a parameter, never in the text. Text that does not parse fails with `INVALID_QUERY`, and the message names the character where it went wrong.

A query that runs many times with different values can be parsed once: `Query::prepare` keeps `$0`, `$1` and on as parameters, and `bind` gives them values without parsing the text again. A prepared query run without values for all its parameters fails with `INVALID_QUERY`.

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["alice@example.com".into()])?;
    let _ = query;
    Ok(())
}
```

A condition on the primary key or on an indexed field, joined to the rest of the filter with `and`, lets the engine read only the objects that meet it. A query sorted by an indexed field alone reads its objects in that order and stops at the limit. Otherwise the engine reads every object of the collection. Whichever way it reads, the result is the same.

## Migrate to a new version

Changing the schema means raising its version. Opening a file that holds an older version migrates it, in one write transaction that either commits whole or leaves the file as it was.

The engine makes some changes by itself: a new collection, a new optional field or one with a default, a removed field, and a new or removed index. Records are not rewritten; an object written before a field existed reads its default, which is why a required field keeps its default once it has one. Anything else is named in a `Migration`:

```rust
use darudb::{Collection, Migration, OpenOptions, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let v2 = Schema::new(2).collection(
        Collection::new("people")
            .field("full_name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::String, "")
            .unique("email"),
    );
    let migration = Migration::to(2)
        .rename_collection("users", "people")
        .rename_field("users", "name", "full_name")
        .replace_field("users", "age")
        .delete_collection("posts")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let before = migrating.previous("users", key.clone())?;
                let age = before.and_then(|user| user.get("age")?.as_int()).unwrap_or(0);
                let mut people = migrating.collection("people")?;

                if let Some(mut person) = people.get(key)? {
                    person.set("age", format!("{age} years"));
                    people.put(person)?;
                }
            }

            Ok(())
        });

    let db = OpenOptions::new().schema(v2).migration(migration).open("app.darudb")?;
    db.close()
}
```

- **Renames** keep the data where it is, so they cost nothing however many objects there are.
- **A replaced field** is a new field with the old name, for a change of type. **A deleted collection** goes with its objects and indexes.
- **The migration function** runs under the new schema. `previous` reads an object as the old schema did, with the old names and the values of removed and replaced fields, so read an object that way before writing it. A deleted collection can still be read that way until the migration commits.
- A function that fails returns its error, and the open fails with it. `Error::MigrationFailed` carries the application's own reason.

Migrations to several versions run in version order. A file two versions behind runs both steps, and one already at the declared version runs none.

## Several processes and handles

Each handle keeps the schema it was opened with. When another process, or another handle in the same process, migrates the file, the next transaction to reach a collection through the old handle fails with `SCHEMA_MISMATCH`, and the handle has to be opened again with the new schema. A read transaction that began before the migration goes on reading under the old schema, since it sees the commit it began at.
