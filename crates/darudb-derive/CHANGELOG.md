# Changelog

> This crate's history. It is released with the `darudb` crate, at the same version, since the code it generates calls that crate; `darudb`'s changelog says what the macros let an application do.

## v1.1.0 (2026-10-09)

### Changed

- Released together with `darudb` 1.1.0, which the code the macros generate calls and which depends on exactly this version. Nothing in the macros changed.

## v1.0.1 (2026-10-07)

### Changed

- Released together with `darudb` 1.0.1, which the code the macros generate calls and which depends on exactly this version. Nothing in the macros changed.

## v1.0.0 (2026-10-05)

### Added

- `#[derive(Object)]`, which implements `darudb::CollectionType` for a struct with named fields, and `#[derive(Embedded)]`, which implements `darudb::EmbeddedType` and `darudb::FieldType`. They read `#[darudb(collection = "...")]` on the struct, and `key`, `index`, `unique`, `rename = "..."` and `default = ...` on a field.
