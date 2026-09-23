# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The engine of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file, and its Rust API.

> DaruDB is in early development. The crate is not published yet, and the file format will change without a migration path until the first release. Only one process may have a file open at a time for now.

## Usage

The storage kernel stores named trees of byte keys and byte values. Typed records and queries come later, built on top of it.

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
