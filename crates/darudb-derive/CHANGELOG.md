# Changelog

> This crate's history. It is released with the `darudb` crate, at the same version, since the code it generates calls that crate; `darudb`'s changelog says what the macros let an application do.

## vNext (2026--)

### Breaking changes

- `#[derive(Embedded)]` declares a constant for each field, as `#[derive(Object)]` does, such as `Address::CITY`, which a path through the embedded object goes on to. A constant of the same name declared by hand on the struct no longer compiles beside it.

### Added

- `#[darudb(index(last, first))]` and `#[darudb(unique(last, first))]` on the struct declare an index on several fields, named as the struct names them, or with a string, a path the collection stores, such as `"address.city"`.
- `#[derive(Embedded)]` implements `darudb::ElementType` too, so that a `Vec` of the struct is a field: a list of embedded objects.

### Changed

- The constants both derives declare hold the field's name in the collection, for the conditions their methods make, and pass it to `FieldOf::new`.

## v1.4.0 (2026-10-29)

### Added

- `#[derive(Object)]` declares an associated constant of the struct for each field, named after the field in capitals and with the field's visibility, such as `Person::AGE` for `age`: a `darudb::FieldOf`, which `TypedWriter::set` and `Change::set` of `darudb` take. A constant of the same name declared by hand on the struct no longer compiles beside it.

## v1.3.0 (2026-10-13)

### Changed

- Released together with `darudb` 1.3.0, which the code the macros generate calls and which depends on exactly this version. Nothing in the macros changed.

## v1.2.0 (2026-10-11)

### Changed

- Released together with `darudb` 1.2.0, which the code the macros generate calls and which depends on exactly this version. Nothing in the macros changed.

## v1.1.0 (2026-10-09)

### Changed

- Released together with `darudb` 1.1.0, which the code the macros generate calls and which depends on exactly this version. Nothing in the macros changed.

## v1.0.1 (2026-10-07)

### Changed

- Released together with `darudb` 1.0.1, which the code the macros generate calls and which depends on exactly this version. Nothing in the macros changed.

## v1.0.0 (2026-10-05)

### Added

- `#[derive(Object)]`, which implements `darudb::CollectionType` for a struct with named fields, and `#[derive(Embedded)]`, which implements `darudb::EmbeddedType` and `darudb::FieldType`. They read `#[darudb(collection = "...")]` on the struct, and `key`, `index`, `unique`, `rename = "..."` and `default = ...` on a field.
