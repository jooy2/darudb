---
title: Constants
order: 13
group: errors
pageClass: reference-page
---

# Constants

The crate exports two constants: the version of the crate and the newest file format version it reads and writes.

## VERSION

```rust
pub const VERSION: &str
```

The version of the `darudb` crate, as its `Cargo.toml` gives it, such as `"1.0.0"`. It is the version of the library compiled into the program, which is useful in a log line or a bug report.

## FORMAT_VERSION

```rust
pub const FORMAT_VERSION: u32
```

The newest file format version this build reads and writes, 6, which a new file gets. Every file records its format version in its header, which [`Database::format_version`](../../api/rust/database.md#format-version) returns, and a file in a version this build does not read is refused with [`UNSUPPORTED_FORMAT_VERSION`](./error.md#unsupportedformatversion), whose `found` field is the file's version.

Any change to what is written to disk changes this number, and every version after 5 comes with a migration from the one before. Version 5 is the format of the first release, and version 6 writes the lengths in a leaf's entries in fewer bytes. This build reads and writes both, and raises a file of version 5 to version 6 when it opens it, unless [`OpenOptions::upgrade_format`](../../api/rust/open-options.md#upgrade-format) says not to; a file a development build wrote before version 5 does not open. The objects of a database are encoded in a format of their own, which the stored schema records; this constant does not cover it. [The file format](../../engine/file-format.md) describes what the version covers.

```rust
fn main() {
    println!("DaruDB {} (file format {})", darudb::VERSION, darudb::FORMAT_VERSION);
}
```
