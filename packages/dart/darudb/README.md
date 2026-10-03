# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The Dart package of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file, for Flutter apps and Dart servers and command-line tools.

> DaruDB is in early development. The package is not published yet, and the file format will change without a migration path until the first release. Until then, the build hook compiles the engine from source, so building an application needs [rustup](https://rustup.rs).

## Usage

Add the package, and `darudb_generator` with `build_runner` for the code they generate:

```yaml
dependencies:
  darudb:
    path: ../darudb/packages/dart/darudb

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator:
    path: ../darudb/packages/dart/darudb_generator
```

Declare each collection as an immutable class, and run `dart run build_runner build`, which writes its schema constant, its query builder and a `copyWith` into the `.g.dart` part:

```dart
import 'package:darudb/darudb.dart';

part 'user.g.dart';

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
```

Open a database with a schema of those constants, and read and write in transactions scoped to a function:

```dart
final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

final id = db.write((txn) => txn.collection(userSchema).insert(const User(name: 'Ada')));

final adults = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q.where(q.age.atLeast(18) & q.name.startsWith('A')).sortBy(q.age, descending: true),
  ),
);

db.write((txn) {
  final users = txn.collection(userSchema);

  users.put(users.get(id)!.copyWith(age: 37));
  users.update(id, (q) => [q.email.set('ada@example.com')]);
});

db.close();
```

`write` commits when its function returns and aborts when it throws, and `durability: Durability.deferred` returns without waiting for the disk. Every failure is a `DaruException` whose `code` is the engine's, the same string in every language DaruDB ships to.

A call of the synchronous API holds the isolate until the engine answers, and a write may wait for another writer or for the disk. The `Future` API does the same work on threads of the native library, so a Flutter app's UI isolate never waits:

```dart
final db = await Database.openAsync('app.darudb', schema: const Schema(1, [userSchema]));

await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);

  await users.insert(const User(name: 'Grace'));
});

final count = await db.readAsync((txn) => txn.collection(userSchema).count());

await db.closeAsync();
```

The calls of one transaction run in the order they were made, awaited or not. An isolate's asynchronous writes on one file take turns, and a synchronous `write`, `sync` or `close` on the file while one runs is refused with `INVALID_ARGUMENT`, since it would hold the isolate the running write needs.
