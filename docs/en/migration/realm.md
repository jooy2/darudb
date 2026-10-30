---
title: Realm
order: 3
languages: [node, dart]
---

# Migrating from Realm

This page moves the data of a Realm file into DaruDB: how object schemas and property types map onto collections and fields, the code that copies every object through Realm's own SDK, in a Node.js script or inside a Flutter app, and what changes in an application that used live objects.

## How the two map

| Realm | DaruDB |
| --- | --- |
| An object schema, or class | A collection |
| An object | An object |
| `primaryKey` | A primary key, of type integer, string or bytes |
| `indexed: true` | An index |
| `bool`, `int`, `float` or `double`, `string`, `data` | A boolean, integer, float, string or bytes field |
| `date` | A date field: `t.date()` read as a `Date`, or a `DateTime` in Dart |
| `objectId` | Its 24 hexadecimal characters as a string, or its 12 bytes |
| `uuid`, `decimal128` | A string field: DaruDB has neither type |
| A link to an object | A link, which holds the other object's primary key |
| A list | A list of values or of links |
| A set | A list, whose values the application keeps distinct |
| A dictionary | No counterpart: an embedded object when its keys are fixed, or JSON text |
| `mixed` | No counterpart: a field for each type it holds, or JSON text |
| An embedded object | An embedded object |
| A list of embedded objects | No counterpart in this version: a collection of its own, linking back to the parent |
| `linkingObjects`, a backlink | Nothing stored: a query on the link field, which an index makes cheap |

- **A link needs a key.** A link holds the primary key of the object it names, so a class that other objects link to needs a primary key. Give one a key with a Realm migration before copying, or copy it as an embedded object when each of its objects belongs to one parent.
- **Defaults are not in the file.** A Realm file records its classes and properties but not their default values, so declare the DaruDB defaults by hand.
- **Large integers.** Through Node.js, the JavaScript SDK reads every `int` as a number, so a value beyond 2^53 has lost precision before the script sees it. The Dart SDK reads it as an `int`, which holds it whole.

## Declare the schema

The example moves this Realm schema: users with an embedded address, and posts that link to them.

::: lang node

```js
const Address = { name: 'Address', embedded: true, properties: { city: 'string', zip: 'string?' } };

const User = {
  name: 'User',
  primaryKey: '_id',
  properties: {
    _id: 'objectId',
    name: 'string',
    email: { type: 'string', optional: true, indexed: true },
    age: { type: 'int', default: 0, indexed: true },
    score: 'double?',
    avatar: 'data?',
    joined: 'date',
    tags: 'string[]',
    address: 'Address?',
    posts: { type: 'linkingObjects', objectType: 'Post', property: 'author' }
  }
};

const Post = {
  name: 'Post',
  primaryKey: 'slug',
  properties: {
    slug: 'string',
    author: 'User?',
    readers: 'User[]',
    price: 'decimal128?',
    ref: 'uuid?',
    meta: 'string{}'
  }
};
```

`_id` becomes a string key named `id`, the date a date field, the `decimal128` and the `uuid` text, and the dictionary JSON text. The backlink `posts` is not copied: an index on `author` makes the posts of a user one query.

The DaruDB schema, in TypeScript:

```ts
import { collection, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    id: t.string().primaryKey(),
    name: t.string(),
    email: t.string().optional().index(),
    age: t.int().default(0).index(),
    score: t.float().optional(),
    avatar: t.bytes().optional(),
    joined: t.date(),
    tags: t.list(t.string()),
    address: t.object({ city: t.string(), zip: t.string().optional() }).optional()
  }),
  posts: collection({
    slug: t.string().primaryKey(),
    author: t.link('users').optional().index(),
    readers: t.list(t.link('users')),
    price: t.string().optional(),
    ref: t.string().optional(),
    meta: t.string()
  })
});
```

:::

::: lang dart

The app's Realm models, from which `realm_generator` made `User`, `Post` and `Address`:

```dart
import 'dart:typed_data';

import 'package:realm/realm.dart';

part 'realm_models.realm.dart';

@RealmModel(ObjectType.embeddedObject)
class _Address {
  late String city;
  String? zip;
}

@RealmModel()
class _User {
  @PrimaryKey()
  @MapTo('_id')
  late ObjectId id;
  late String name;
  @Indexed()
  String? email;
  @Indexed()
  int age = 0;
  double? score;
  Uint8List? avatar;
  late DateTime joined;
  late List<String> tags;
  _Address? address;
  @Backlink(#author)
  late Iterable<_Post> posts;
}

@RealmModel()
class _Post {
  @PrimaryKey()
  late String slug;
  _User? author;
  late List<_User> readers;
  Decimal128? price;
  Uuid? ref;
  late Map<String, String> meta;
}
```

`id` becomes a string key, the date a `DateTime` field, the `Decimal128` and the `Uuid` text, and the map JSON text. The backlink `posts` is not copied: an index on `author` makes the posts of a user one query.

The `realm` package depends on `realm_generator`, which pins an older `analyzer` and `source_gen` than `darudb_generator` needs, so the two generators do not resolve in one package. The DaruDB classes go in a package of their own inside the app's repository, `packages/app_data`, with `darudb_generator` among its `dev_dependencies`:

```yaml
name: app_data
publish_to: none

environment:
  sdk: ^3.10.0

dependencies:
  darudb: ^1.4.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.1.0
```

```dart
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'app_data.g.dart';

@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  final String? zip;
}

@Collection('users')
class User {
  const User({
    required this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.score,
    this.avatar,
    required this.joined,
    required this.tags,
    this.address,
  });

  @PrimaryKey()
  final String id;
  final String name;
  @Index()
  final String? email;
  @Index()
  final int age;
  final double? score;
  final Uint8List? avatar;
  final DateTime joined;
  final List<String> tags;
  final Address? address;
}

@Collection('posts')
class Post {
  const Post({
    required this.slug,
    this.author,
    required this.readers,
    this.price,
    this.ref,
    required this.meta,
  });

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
  final List<Link<User>> readers;
  final String? price;
  final String? ref;
  final String meta;
}

const app = Schema(1, [userSchema, postSchema]);
```

`dart run build_runner build` in `packages/app_data` writes `app_data.g.dart`. The app depends on the package by path, beside `realm`, and keeps running `realm_generator` for its own models:

```yaml
dependencies:
  realm: ^20.2.0
  app_data:
    path: packages/app_data
```

Once a later version of the app no longer opens the Realm file, `realm` goes, and the classes can move back into the app with `darudb_generator`.

:::

## Copy the objects

::: lang node

The script reads the Realm file through the `realm` package and writes the DaruDB file through the Node.js package:

```js
import Realm from 'realm';
import { Database } from 'darudb';

const realm = await Realm.open({ path: 'app.realm', readOnly: true });
const db = Database.open('app.darudb', { schema: app });

/** Copies `objects` into collection `name`, a thousand to a transaction. */
function copy(objects, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for (const object of objects) {
    batch.push(convert(object));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

copy(realm.objects('User'), 'users', (user) => ({
  id: user._id.toHexString(),
  name: user.name,
  email: user.email,
  age: user.age,
  score: user.score,
  avatar: user.avatar === null ? null : new Uint8Array(user.avatar),
  joined: user.joined,
  tags: [...user.tags],
  address: user.address === null ? null : { city: user.address.city, zip: user.address.zip }
}));

copy(realm.objects('Post'), 'posts', (post) => ({
  slug: post.slug,
  author: post.author === null ? null : post.author._id.toHexString(),
  readers: [...post.readers].map((user) => user._id.toHexString()),
  price: post.price === null ? null : post.price.toString(),
  ref: post.ref === null ? null : post.ref.toString(),
  meta: JSON.stringify(post.meta)
}));

// Every deferred commit is durable once this returns.
db.close();
realm.close();

// The `realm` package keeps Node.js running after `close`.
process.exit(0);
```

- **Read-only, with the file's own schema.** Opened with `readOnly` and no schema, the file is read as it is and stays as it was. An encrypted file also needs its `encryptionKey`.
- **Classes, not embedded objects.** `realm.objects` takes a class; an embedded object is copied with its parent, as `address` is here.
- **One class at a time.** A link holds the key whatever has been copied so far, so the classes can go in any order.

:::

::: lang dart

A Flutter app's Realm file is on each device, so the copy runs in the app itself, once, at the first launch of the version that moves, through the app's own Realm models:

```dart
import 'dart:convert';
import 'dart:io';

import 'package:app_data/app_data.dart';
import 'package:darudb/darudb.dart';
import 'package:realm/realm.dart' as realm;

import 'realm_models.dart' as old;

/// Copies the Realm file at [from] into a DaruDB file at [path], once.
Future<void> moveFromRealm(String from, String path) async {
  final part = File('$path.part');

  if (File(path).existsSync()) {
    return;
  }

  // A copy a crash cut short is started again.
  if (part.existsSync()) {
    part.deleteSync();
  }

  final source = realm.Realm(
    realm.Configuration.local(
      [old.User.schema, old.Post.schema, old.Address.schema],
      path: from,
      isReadOnly: true,
    ),
  );
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
    source.all<old.User>(),
    userSchema,
    (old.User user) => User(
      id: user.id.hexString,
      name: user.name,
      email: user.email,
      age: user.age,
      score: user.score,
      avatar: user.avatar,
      joined: user.joined,
      tags: List.of(user.tags),
      address: user.address == null
          ? null
          : Address(city: user.address!.city, zip: user.address!.zip),
    ),
  );

  await copy(
    source.all<old.Post>(),
    postSchema,
    (old.Post post) => Post(
      slug: post.slug,
      author: post.author == null ? null : Link<User>(post.author!.id.hexString),
      readers: [for (final user in post.readers) Link<User>(user.id.hexString)],
      price: post.price?.toString(),
      ref: post.ref?.toString(),
      meta: jsonEncode(Map.of(post.meta)),
    ),
  );

  // Every deferred commit is durable once this returns.
  await db.closeAsync();
  source.close();
  part.renameSync(path);
}
```

The app calls it before it opens the database, with the path of its Realm file and a path for the new one in a folder that the system does not clear, such as the one `getApplicationSupportDirectory` from `path_provider` gives:

```dart
final folder = await getApplicationSupportDirectory();
final path = '${folder.path}/app.darudb';

await moveFromRealm(realm.Configuration.defaultRealmPath, path);

final db = await Database.openAsync(path, schema: app);
```

- **Read-only, through the app's models.** Opened with `isReadOnly`, the file stays as it was. An encrypted file also needs its `encryptionKey` in the configuration.
- **Copied under another name, then moved into place.** The copy is written to `app.darudb.part` and renamed once it is closed, so the app finds either no database, and copies again, or a complete one.
- **Off the UI's way.** The engine's writes run on threads of the native library through the `Future` API, so the app can show a screen that says it is updating while the copy runs.
- **Keep the Realm file for a while.** Delete it with `realm.Realm.deleteRealm` in a later version, once the app has run on the new file.
- **Imported under a prefix.** The Realm models and the DaruDB classes have the same names, so the copy imports the models as `old`.

:::

## Check the copy

Count the objects of each class through Realm, `objects('User').length` in Node.js or `all` and `length` in Dart, and compare them with `count` on each collection, and run the [integrity check](../guide/tools.md#check-a-file) on the new file.

## Queries

The query language is close to the Realm Query Language that `filtered` takes:

| Realm Query Language                     | DaruDB query language                     |
| ---------------------------------------- | ----------------------------------------- |
| `age >= $0`                              | `age >= $0`                               |
| `email == nil`                           | `email IS NULL`                           |
| `age BETWEEN {18, 30}`                   | `age BETWEEN 18 AND 30`                   |
| `age IN {18, 30}`                        | `age IN [18, 30]`                         |
| `name BEGINSWITH 'A'`                    | `name STARTSWITH "A"`                     |
| `name CONTAINS 'li'`, `ENDSWITH`         | `name CONTAINS "li"`, `ENDSWITH`          |
| `ANY tags == 'red'`                      | `tags == "red"`, or `tags CONTAINS "red"` |
| `author.name == 'Alice'`                 | `author.name == "Alice"`                  |
| `TRUEPREDICATE SORT(age DESC) LIMIT(10)` | `SORT BY age DESC LIMIT 10`               |

- The `[c]` of `CONTAINS[c]` is `IGNORE CASE` after the condition: `name CONTAINS "li" IGNORE CASE`. `LIKE` with wildcards has no counterpart.
- The aggregates `@sum`, `@min`, `@max` and `@avg` of a field become `sum`, `min`, `max` and `avg` on the collection, with the query, and `@count` becomes `count`. An aggregate inside a filter, such as `items.@sum.price > 10`, has no counterpart.
- A backlink becomes a query on the link field: the posts of a user are `author == $0` on `posts`, with the user's key. A filter on a backlink, such as `@links.Post.author.year > 2020`, becomes `ANY posts.author (year > 2020)` on `users`.

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **Results are plain objects.** A query returns objects copied out of the file, not live objects tied to it: they do not change when the data does, and they stay usable after the transaction and the database are closed. A change is written back with `put` or `update`.
- **No change notifications.** Nothing calls the application when data changes. After a write, read again what the screen shows.
- **Transactions** are <LangCode node="db.write" dart="db.write and db.writeAsync" /> instead of `realm.write`, and reads happen in a read transaction too. See [Transactions](../guide/transactions.md).
- **Schema versions** keep working the same way: raise the schema's version, and give a migration for what the engine does not do by itself. See [Migrations](../guide/migrations.md).
- **Encryption** takes a 32-byte key or a password instead of Realm's 64-byte key. The new file has a key of its own; see [Encryption](../guide/encryption.md).
- **Sync** to a server has no counterpart: DaruDB keeps data in a local file only.
- **Speed** is measured beside Realm on the [performance page](../performance.md), in its Node.js and Dart tables, on the same objects and workloads.
