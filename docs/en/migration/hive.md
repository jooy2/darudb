---
title: Hive
order: 8
languages: [dart]
---

# Migrating from Hive

This page moves the data of a Flutter app's Hive boxes into DaruDB: how boxes, keys and type adapters map onto collections, objects and fields, the code that copies every entry inside the app, and what changes in the app.

## How the two map

| Hive | DaruDB |
| --- | --- |
| A box: one `.hive` file each | A collection, in one database file for all of them |
| An `int` key, such as `add` gives | The automatic `id`, an integer |
| A `String` key, such as `put` takes | A string primary key |
| A class with `@HiveType` and its generated adapter | A class with `@Collection()`, whose code `darudb_generator` generates |
| A field with `@HiveField` | A field of the class; the engine numbers the fields it stores itself |
| A field that holds another entry's key | A `Link`, which holds that object's key |
| `HiveList` | A list of links |
| A class nested in another, with an adapter of its own | An embedded object, a class with `@Embedded()` |
| `HiveObject`, with `save` and `delete` | No counterpart: `put` and `delete` in a write transaction |
| `LazyBox` | A collection: every read comes from the file, through the page cache |
| `box.watch`, `ValueListenableBuilder` on `box.listenable()` | No counterpart: read again after a write |
| `HiveAesCipher` | Encryption with a key or a password, of the whole file |
| `compact` | `compact` |

- **One kind of key per collection.** A box can mix `int` and `String` keys; a collection's primary key has one type. A box that mixes them becomes a collection with a string key, its integer keys written as text, and every field that held one of them converted the same way.
- **Keys carry over.** Copying each key as the object's key keeps every reference between entries valid as a link. Hive's own keys start at 0, and the collection goes on numbering after the largest one copied into it.
- **Indexes are new.** Hive finds an entry by its key, and a box is searched by walking its values in memory. Declare an index on each field the app looks entries up by, and the engine reads only the objects that match.

## Declare the schema

The example moves two boxes of a Flutter app: `users`, whose entries the app added with `add`, and `posts`, which it put under their slug. These are the app's Hive classes:

```dart
import 'package:hive_ce/hive_ce.dart';

part 'hive_models.g.dart';

@HiveType(typeId: 0)
class User {
  User({required this.name, this.email, this.age = 0});

  @HiveField(0)
  final String name;
  @HiveField(1)
  final String? email;
  @HiveField(2)
  final int age;
}

@HiveType(typeId: 1)
class Post {
  Post({this.authorKey, required this.body, this.publishedAt, required this.tags});

  /// The key of a user in the `users` box.
  @HiveField(0)
  final int? authorKey;
  @HiveField(1)
  final String body;
  @HiveField(2)
  final String? publishedAt;
  @HiveField(3)
  final List<String> tags;
}
```

The DaruDB classes go in a file of their own, `schema.dart`, since their names are the same. `users` keeps Hive's keys as the automatic `id`, and `authorKey` becomes a link:

```dart
import 'package:darudb/darudb.dart';

part 'schema.g.dart';

@Collection('users')
class User {
  const User({this.id, required this.name, this.email, this.age = 0});

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
}

@Collection('posts')
class Post {
  const Post({
    required this.slug,
    this.author,
    required this.body,
    this.publishedAt,
    required this.tags,
  });

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
  final String body;
  final String? publishedAt;
  @Index()
  final List<String> tags;
}

const app = Schema(1, [userSchema, postSchema]);
```

`darudb_generator` and `hive_ce_generator` run together in one `build_runner build`:

```yaml
dependencies:
  darudb: ^1.4.0
  hive_ce: ^2.11.0
  hive_ce_flutter: ^2.3.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.1.0
  hive_ce_generator: ^1.11.0
```

An app on the original `hive` package cannot do the same: `hive_generator` pins an older `analyzer` and `source_gen` than `darudb_generator` needs, so the two do not resolve in one package. Keep the DaruDB classes in a package of their own, with `darudb_generator` among its `dev_dependencies`, and depend on it by path from the app, as the [Realm guide](./realm.md#declare-the-schema) does.

## Copy the entries

The boxes are on each device, so the copy runs in the app itself, once, at the first launch of the version that moves, before the app opens its boxes:

```dart
import 'dart:io';

import 'package:darudb/darudb.dart';
import 'package:hive_ce_flutter/hive_flutter.dart';

import 'hive_models.dart' as hive;
import 'schema.dart';

/// Copies the Hive boxes into a DaruDB file at [path], once.
Future<void> moveFromHive(String path) async {
  final part = File('$path.part');

  if (File(path).existsSync()) {
    return;
  }

  // A copy a crash cut short is started again.
  if (part.existsSync()) {
    part.deleteSync();
  }

  await Hive.initFlutter();
  Hive.registerAdapter(hive.UserAdapter());
  Hive.registerAdapter(hive.PostAdapter());

  final users = await Hive.openBox<hive.User>('users');
  final posts = await Hive.openBox<hive.Post>('posts');
  final db = await Database.openAsync(part.path, schema: app);

  /// Copies [objects] into a collection, a thousand to a transaction.
  Future<void> copy<R, T>(
    Iterable<R> objects,
    CollectionSchema<T, QueryBuilder<T>, Object> collection,
    T Function(R object) convert,
  ) async {
    final batch = <T>[];

    Future<void> flush() async {
      await db.writeAsync(
        (txn) => txn.collection(collection).insertMany(batch),
        durability: Durability.deferred,
      );
      batch.clear();
    }

    for (final object in objects) {
      batch.add(convert(object));

      if (batch.length == 1000) {
        await flush();
      }
    }

    if (batch.isNotEmpty) {
      await flush();
    }
  }

  await copy(
    users.toMap().entries,
    userSchema,
    (MapEntry<dynamic, hive.User> entry) => User(
      id: entry.key as int,
      name: entry.value.name,
      email: entry.value.email,
      age: entry.value.age,
    ),
  );

  await copy(
    posts.toMap().entries,
    postSchema,
    (MapEntry<dynamic, hive.Post> entry) => Post(
      slug: entry.key as String,
      author: entry.value.authorKey == null ? null : Link<User>(entry.value.authorKey!),
      body: entry.value.body,
      publishedAt: entry.value.publishedAt,
      tags: entry.value.tags,
    ),
  );

  // Every deferred commit is durable once this returns.
  await db.closeAsync();
  await Hive.close();
  part.renameSync(path);
}
```

The app calls it before it opens the database, with a path in a folder that the system does not clear, such as the one `getApplicationSupportDirectory` from `path_provider` gives:

```dart
final folder = await getApplicationSupportDirectory();
final path = '${folder.path}/app.darudb';

await moveFromHive(path);

final db = await Database.openAsync(path, schema: app);
```

- **Copied under another name, then moved into place.** The copy is written to `app.darudb.part` and renamed once it is closed, so the app finds either no database, and copies again, or a complete one. The boxes are not changed, so a copy cut short loses nothing.
- **Off the UI's way.** The engine's writes run on threads of the native library through the `Future` API, so the app can show a screen that says it is updating while the copy runs.
- **Keep the boxes for a while.** Delete them with `Hive.deleteBoxFromDisk` in a later version, once the app has run on the new file.

## Check the copy

Compare `users.length` on each box with `count` on each collection, before the boxes are deleted, and run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

What the app did by walking a box's values becomes a query, which the engine runs through an index when one fits:

| What the app did | DaruDB, on the collection |
| --- | --- |
| `box.get(key)` | `get(key)` |
| `box.values.where((user) => user.age >= 18)` | `find` with `q.where(q.age.atLeast(18))` |
| `box.values.firstWhere((user) => user.email == email)` | `findOne` with `q.where(q.email.equals(email))`, through the unique index |
| `box.values.where((post) => post.tags.contains('news'))` | `find` with `q.where(q.tags.contains('news'))`, on `posts` |
| Sorting the values, then `take(10)` | `find` with `q.sortBy(q.age, descending: true).limit(10)` |
| `box.length` | `count()` |

[Queries](../guide/queries.md) has the whole builder, and the query language.

## What changes in the app

- **Reads come from the file**, through the page cache, not from a box held in memory. A read by key is a lookup in the file's tree, and the app no longer needs the memory of every box at once.
- **Writes are transactions.** `db.write` and `db.writeAsync` commit when their function returns and abort when it throws, so several changes land together or not at all. See [Transactions](../guide/transactions.md).
- **Objects are immutable**: a change is a `copyWith` written back with `put`, or `update` of the fields it sets.
- **No listeners.** Nothing calls the app when data changes. After a write, read again what the screen shows.
- **Several isolates and processes** can use one file at the same time, such as an app and its extension: one writer, any number of readers. See [Several processes](../guide/processes.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
