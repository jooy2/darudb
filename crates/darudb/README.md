# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The engine of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file, and its Rust API.

## Installation

```bash
cargo add darudb
```

Add `--features derive` for `#[derive(Object)]` and `#[derive(Embedded)]`, which make structs the objects of a collection ([below](#objects-as-rust-types)).

## Usage

The storage kernel stores named trees of byte keys and byte values, and a database opened with a schema also holds collections of typed objects with indexes, queries and migrations ([below](#collections-and-queries)).

```rust
use darudb::Database;

fn main() -> Result<(), darudb::Error> {
    // Opens the database, creating the file if it does not exist.
    let db = Database::open("app.darudb")?;

    // Every change in a write transaction becomes visible, and durable,
    // together when `commit` returns.
    let mut txn = db.begin_write()?;
    txn.insert("users", b"alice", b"admin")?;
    txn.insert("users", b"bob", b"member")?;
    txn.commit()?;

    // A read transaction sees one commit for as long as it lives.
    let read = db.begin_read()?;
    assert_eq!(read.get("users", b"alice")?, Some(b"admin".to_vec()));

    for entry in read.iter("users")? {
        let (key, value) = entry?;
        println!("{} = {}", String::from_utf8_lossy(&key), String::from_utf8_lossy(&value));
    }

    Ok(())
}
```

A commit is durable when `commit` returns, and a file opened after a crash or a power cut holds the last commit that returned. `commit_deferred` returns without waiting for the disk: readers see the changes at once, a crash of the process loses none of them, and they become durable within a second, or sooner at the next `commit`, `Database::sync` or close. Every error carries a stable code, `Error::code`, which is the same string in every language DaruDB ships to.

Several processes can open the same file at once. One writes at a time, readers never wait for it, and a process that dies at any moment leaves nothing for the others to clean up. A process that has a database open must not open the file any other way, not even to copy it: on Unix-like systems, closing that second handle drops the locks the database holds.

### Collections and queries

A schema declares collections, their fields, their primary keys and their indexes, at a version. The engine stores it in the file, keeps every index in step with the objects in the same transaction, and migrates the file when the version rises.

```rust
use darudb::{Collection, Filter, Object, OpenOptions, Query, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1).collection(
        Collection::new("users")
            .field("name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::Int, 0)
            .unique("email")
            .index("age"),
    );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;
    users.insert(Object::new().with("name", "Alice").with("age", 31))?;
    users.insert(Object::new().with("name", "Bob").with("email", "bob@example.com"))?;
    txn.commit()?;

    let read = db.begin_read()?;
    let adults = read.collection("users")?.query(
        &Query::new().filter(Filter::ge("age", 18)).sort_by_desc("age").limit(10),
    )?;
    let same = Query::parse("age >= $0 SORT BY age DESC LIMIT 10", &[18.into()])?;

    assert_eq!(read.collection("users")?.query(&same)?, adults);
    Ok(())
}
```

A query reads through the primary key or an index when a condition of its filter allows it, and gives the same objects either way. Results are plain objects that outlive the transaction.

### Objects as Rust types

With the `derive` feature, `#[derive(Object)]` makes a struct the objects of a collection. A typed read decodes the record straight into the struct, without the `Object` of named values in between.

```toml
[dependencies]
darudb = { version = "1.0", features = ["derive"] }
```

```rust
use darudb::{Collection, Filter, Object, OpenOptions, Query, Schema};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
}

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .schema(Schema::new(1).collection(Collection::of::<User>()))
        .open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let id = txn.collection_of::<User>()?.insert(&User {
        id: None,
        name: "Alice".to_owned(),
        email: None,
        age: 31,
    })?;
    txn.commit()?;

    let read = db.begin_read()?;
    let users = read.collection_of::<User>()?;

    assert_eq!(users.get(id)?.map(|user| user.name), Some("Alice".to_owned()));

    for user in users.query(&Query::new().filter(Filter::ge("age", 18)))? {
        println!("{user:?}");
    }

    Ok(())
}
```

`Option` fields are optional, `Vec<T>` is a list, `Link<T>` a link to another collection's object, and a struct with `#[derive(Embedded)]` an embedded object. A struct whose fields do not match the stored collection fails with `INVALID_ARGUMENT` when a transaction reaches the collection through it.

### Encryption

A database created with a key or a password is encrypted, every page of it, and cannot be opened without it:

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    // Changing the password encrypts no page again.
    db.set_password("a new password")?;

    Ok(())
}
```

Opening it without a password fails with the code `KEY_REQUIRED`, and with the wrong one with `WRONG_KEY`. `OpenOptions::key` takes a 32-byte key instead, for one kept in the operating system's keystore.

## Requirements

Rust 1.85 or later, on a Unix-like system or Windows.

## License

[MIT](https://github.com/jooy2/darudb/blob/main/LICENSE) © [CDGet](https://cdget.com)
