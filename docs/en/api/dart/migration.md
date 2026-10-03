---
title: Migration
order: 4
counterpart: /types/node/migration
---

# Migration

`Migration` says what schema version `version` changes from the version before it, beyond what the engine works out by itself.

```dart
final class Migration {
  const Migration(
    this.version, {
    this.renameCollections = const {},
    this.renameFields = const {},
    this.deleteCollections = const [],
    this.replaceFields = const {},
    this.run,
  });
}
```

The `migrations` option of [`Database.open`](./database.md#open) takes a list of them. Opening a file that holds an older schema version runs every version step up to the declared one, in version order, in one write transaction. If any part fails, the file keeps its old schema and data.

The engine makes some changes by itself: it creates new collections, adds new fields that are optional or have a default, builds new indexes and drops removed ones, and retires removed fields. A migration names the rest: a renamed collection or field, a deleted collection, and a field whose type changed. A collection that is gone or a field whose type changed, left unnamed, fails with `INVALID_ARGUMENT`, and so does a name the schema before the step does not have. A field renamed without `renameFields` is taken as a removed field and a new one, so its values do not follow it to the new name. Every name in a step is the name in the file before that step, so a field of a collection the step renames is named under the collection's old name, and a field annotated `@Name` by the name it has in the file.

This migration takes a file from version 1, where `users` held `name` and an int `age`, to version 2:

```dart
@Collection('people')
class Person {
  const Person({this.id, required this.fullName, this.age = ''});

  final int? id;
  final String fullName;
  final String age;
}

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
          final before = m.previous('users', key);
          final person = people.get(key as int);

          if (before != null && person != null) {
            people.put(person.copyWith(age: '${before['age']} years'));
          }
        }
      },
    ),
  ],
);
```

## Properties

### version

```dart
final int version;
```

The schema version this step leads to: a whole number from 2 up to the declared schema's version. Any other number, or two steps to the same version, fails with `INVALID_ARGUMENT`.

### renameCollections

```dart
final Map<String, String> renameCollections;
```

New collection names, by old name. A rename moves no data, so it costs nothing however many objects the collection holds.

### renameFields

```dart
final Map<String, Map<String, String>> renameFields;
```

For each collection, by its name before the step, new field names by old name.

### deleteCollections

```dart
final List<String> deleteCollections;
```

Collections this step deletes, with their objects and indexes, after `run` has run. Until the migration commits, `run` can still read their objects with `previous`.

### replaceFields

```dart
final Map<String, List<String>> replaceFields;
```

For each collection, by its name before the step, the fields whose type changes. A replaced field is a field removed and a new one added under the same name, so it takes its default, or null, until `run` gives it a value.

### run

```dart
final FutureOr<void> Function(MigrationContext context)? run;
```

Moves data across, in the migration's write transaction, after the step's renames and the engine's own changes. It receives a [MigrationContext](./migration-context.md). Through `Database.open` it has to be synchronous; through `Database.openAsync` it may be asynchronous, and the step ends once its `Future` completes. If it throws, `open` throws the same error.
