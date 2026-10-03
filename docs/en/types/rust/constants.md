---
title: Constants
order: 13
---

# Constants

The crate exports two constants: the version of the crate and the file format version it reads and writes.

## VERSION

```rust
pub const VERSION: &str
```

The version of the `darudb` crate, as its `Cargo.toml` gives it, such as `"0.1.0"`. It is the version of the library compiled into the program, which is useful in a log line or a bug report.

## FORMAT_VERSION

```rust
pub const FORMAT_VERSION: u32
```

The file format version this build reads and writes. Every file records its format version in its header, and a file in any other version is refused with [`UNSUPPORTED_FORMAT_VERSION`](./error.md#unsupportedformatversion), whose `found` field is the file's version. [`Database::format_version`](../../api/rust/database.md) returns the same number.

Any change to what is written to disk changes this number, and until the first release there are no migrations between format versions: a file written by a build with another format version does not open. The objects of a database are encoded in a format of their own, which the stored schema records; this constant does not cover it. [The file format](../../engine/file-format.md) describes what the version covers.

```rust
fn main() {
    println!("DaruDB {} (file format {})", darudb::VERSION, darudb::FORMAT_VERSION);
}
```
