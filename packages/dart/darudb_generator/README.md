# darudb_generator

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The code generator of [DaruDB](https://darudb.cdget.com)'s Dart package. With `build_runner`, it writes for each class annotated `@Collection()` its schema constant, such as `userSchema` for `User`, the functions that write it as a record and read it from one, a query builder with a typed field for each field, a link type for queries through links, and a `copyWith`; and for each class annotated `@Embedded()` the same, for an embedded object.

```yaml
dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator:
    path: ../darudb/packages/dart/darudb_generator
```

```bash
dart run build_runner build
```

A class it reads has `final` fields of a type the database stores, `bool`, `int`, `double`, `String`, `Uint8List`, a `List` of those or of links, a `Link` to a collection or a class annotated `@Embedded()`, each nullable for an optional field, and an unnamed constructor that takes every field. A constructor parameter's default is the field's default in the schema. The `darudb` package documents the annotations.
