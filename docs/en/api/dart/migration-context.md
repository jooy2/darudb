---
title: MigrationContext
order: 5
counterpart: [/api/rust/migrating, /api/node/migrating]
---

# MigrationContext

`MigrationContext` is what a migration function gets: the collections of the new schema, and the objects as the schema before the migration read them.

```dart
final class MigrationContext
```

When `Database.open` finds the file at an older schema version, it migrates the file in one write transaction. It applies every step's renames and the changes the engine makes by itself, then calls the `run` function of each [Migration](./migration.md) between the two versions, in version order, with a `MigrationContext`. If a function throws, nothing of the migration is kept, the file keeps its old schema and data, and `open` throws the same error. [Migrations](../../guide/migrations.md) explains what the engine changes by itself and what needs a function.

Its calls are synchronous, through `Database.openAsync` too, where the function itself may be asynchronous. The migration holds the file's writer until it commits, so nothing a call waits for is another writer of the file; a function should not write to the same file through another handle, which would wait for the migration. The context and its collections can be used only while the migration runs; afterwards every call throws `CLOSED`.

```dart
final db = Database.open(
  'app.darudb',
  schema: const Schema(2, [personSchema]),
  migrations: [
    Migration(
      2,
      renameCollections: const {'users': 'people'},
      renameFields: const {
        'users': {'name': 'fullName'},
      },
      replaceFields: const {
        'users': ['age'],
      },
      run: (m) {
        final people = m.collection(personSchema);

        for (final key in m.previousKeys('users')) {
          final before = m.previous('users', key)!;

          people.update(key as int, (q) => [q.age.set('${before['age']} years')]);
        }
      },
    ),
  ],
);
```

`Person` is the class of [Migration](./migration.md)'s example, whose `age` became a string.

## Properties

### previousVersion

```dart
int get previousVersion;
```

The schema version the file held before the migration. It is the same in every step.

## Methods

### collection

```dart
WriteCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> schema,
);
```

The collection of `schema` in the new schema, as a [WriteCollection](./write-collection.md) that reads and writes the migration's transaction. Its queries are built with the query builder: `findText` and `countText` fail with `INVALID_ARGUMENT` in a migration. A collection the new schema does not have fails with `INVALID_ARGUMENT`.

### previous

```dart
Map<String, Object?>? previous(String name, Object key);
```

The object of collection `name` whose primary key is `key`, as the schema before the migration reads it, or `null`. The collection and the object's fields have the names they had in the file before the migration, and the fields the migration removed or replaced keep their values. A field the record leaves out holds its default, or null. An embedded object comes back as a map of its own, a link as the key it holds, and a list as a `List`. A collection the migration deletes can still be read this way until it commits. A collection the old schema did not have fails with `INVALID_ARGUMENT`.

It reads the object as it is now, and writing an object keeps only the new schema's fields, so read an object this way before writing it. The class of the old schema is usually gone from the program, which is why the object is a map.

### previousKeys

```dart
List<Object> previousKeys(String name);
```

The primary keys of every object of collection `name`, named as before the migration, in key order: `int`, `String` or `Uint8List` values, as the collection's key type is.
