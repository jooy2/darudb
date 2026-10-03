# darudb-derive

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The derive macros of [DaruDB](https://darudb.cdget.com): `#[derive(Object)]`, which makes a struct the objects of a collection, and `#[derive(Embedded)]`, which makes one an embedded object.

Use them through the `darudb` crate, with its `derive` feature, rather than depending on this crate:

```toml
[dependencies]
darudb = { version = "0.1", features = ["derive"] }
```

The `darudb` crate documents both macros and the attributes they read.
