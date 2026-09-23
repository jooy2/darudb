# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The engine of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file, and its Rust API.

> DaruDB is in early development. The crate is not published yet, it cannot store data yet, and the file format will change without a migration path until the first release.

## Usage

```rust
use darudb::{Database, OpenOptions};

fn main() -> Result<(), darudb::Error> {
    // Opens the database, creating the file if it does not exist.
    let db = Database::open("app.darudb")?;
    println!("page size: {} bytes", db.page_size());
    db.close()?;

    // Refuses to create a file, and fails with `NOT_FOUND` instead.
    let db = OpenOptions::new().create(false).open("app.darudb")?;
    db.close()
}
```

Every error carries a stable code, `Error::code`, which is the same string in every language DaruDB ships to.

## Requirements

Rust 1.85 or later, on a Unix-like system or Windows.

## License

[MIT](https://github.com/jooy2/darudb/blob/main/LICENSE) © [CDGet](https://cdget.com)
