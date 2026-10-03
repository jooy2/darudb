---
title: Schema
order: 3
---

# Schema

`Schema` declares a database's collections at a version, from the schema constants `darudb_generator` writes, for `Database.open` to take.

```dart
final class Schema {
  const Schema(this.version, this.collections);

  final int version;
  final List<CollectionSchema<Object?, QueryBuilder<Object?>, Object>> collections;
}
```

`version` is a whole number from 1 up; raise it whenever the schema changes. `collections` lists the [CollectionSchema](../../types/dart/collection-schema.md) of each collection, such as `userSchema` for a class `User` annotated `@Collection()`. Every collection a link points to is in the list too. A schema is a constant, so any number of opens can share one.

```dart
const app = Schema(1, [teamSchema, userSchema]);

final db = Database.open('app.darudb', schema: app);
```

A schema the engine cannot store fails to open with `INVALID_ARGUMENT`: a version below 1, two collections with the same name, or a link to a collection the schema does not have. The generator refuses the rest before the program runs, such as a collection with two primary keys or an index inside an embedded object.

The first open stores the schema in the file, and every later open compares the declared schema with the stored one:

- **The same version and the same schema**: nothing to do. Listing collections or declaring fields in another order is not a change, since fields are matched by name.
- **The same version and another schema**: `SCHEMA_MISMATCH`, since the schema changed without a new version.
- **A newer version in the file**: `SCHEMA_TOO_NEW`, since a newer application wrote it.
- **An older version in the file**: a migration, which [Migration](./migration.md) and the guide's [Migrations](../../guide/migrations.md) explain.

## Properties

### version

```dart
final int version;
```

The schema version.

### collections

```dart
final List<CollectionSchema<Object?, QueryBuilder<Object?>, Object>> collections;
```

The collections, in the order they were given.
