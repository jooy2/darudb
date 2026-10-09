# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The Dart package of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file, for Flutter apps and Dart servers and command-line tools.

## Usage

Add the package, and `darudb_generator` with `build_runner` for the code they generate:

```yaml
dependencies:
  darudb: ^1.0.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.0.0
```

The package needs Dart 3.10 or later, or Flutter 3.38.1 or later, the first releases with stable build hooks. Flutter 3.38.0 ships a beta build of Dart 3.10, which the package does not accept. Its build hook downloads the engine prebuilt for the application's target, so building an application needs no Rust toolchain; [the native library](#the-native-library) says how.

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

On Dart 3.10, run `dart run build_runner build --force-jit`: the newest `build_runner` there compiles its builders with `dart compile`, which Dart 3.10 refuses for a project with this package's build hook, and stops with `'dart compile' does not support build hooks`.

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

## The native library

The package's build hook gives the application the engine as a native library for its target, in one of two ways:

- **A published copy** downloads the library built for the target from the package's GitHub release, the first time it builds for that target, and keeps it in the hook's cache. It uses the file only if its SHA-256 hash is the one `hook/prebuilt.json` in the package names, so the package vouches for what it loads. A build without network access needs the library in the cache already. Libraries ship for Android on arm64, armv7 and x86_64, for iOS devices and simulators, and for macOS, Windows and Linux (glibc 2.17 or later) on arm64 and x86_64. They are dynamic libraries, so a build that requires a static one needs a checkout of the repository.
- **A checkout of the repository**, which an application gets by depending on the package through git or a path, builds the engine from the Rust crate in `native/` with the compiler `native/rust-toolchain.toml` pins, which [rustup](https://rustup.rs) installs on the first build. The first build takes a minute or two.
