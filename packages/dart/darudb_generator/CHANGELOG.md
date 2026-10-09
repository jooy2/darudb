# Changelog

> This package's history. It generates code for the `darudb` package, whose changelog says what that code lets an application do.

## v1.1.0 (2026-10-09)

### Added

- An example, `example/README.md`, of a class the generator reads and the code an application writes with what it generates.

### Changed

- Versioned 1.1.0 together with `darudb` 1.1.0. The code it generates is the same as 1.0.0's, and works with `darudb` 1.0 and 1.1 alike.

## v1.0.0 (2026-10-05)

### Added

- The builder `darudb`, which writes for each class annotated `@Collection()` its schema constant, record codec, query builder, link type and `copyWith`, and for each class annotated `@Embedded()` its schema constant, codec, query fields and `copyWith`, into the library's `.g.dart` part. It refuses a class it cannot store with an error that names the field: a field that is not final, a type the database does not store, a nullable field with a default, two primary keys, a collection without a key and without `int? id`, or an index inside an embedded object.
- Runs on Dart 3.10 and later, with `analyzer` from 12.1 and `source_gen` from 4.2.4, the newest of each that resolves on Dart 3.10.
