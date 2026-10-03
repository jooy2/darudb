# Changelog

> This package's history. It generates code for the `darudb` package, whose changelog says what that code lets an application do.

## vNext (2026--)

### Added

- The builder `darudb`, which writes for each class annotated `@Collection()` its schema constant, record codec, query builder, link type and `copyWith`, and for each class annotated `@Embedded()` its schema constant, codec, query fields and `copyWith`, into the library's `.g.dart` part. It refuses a class it cannot store with an error that names the field: a field that is not final, a type the database does not store, a nullable field with a default, two primary keys, a collection without a key and without `int? id`, or an index inside an embedded object.
