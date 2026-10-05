# Example

An application depends on `darudb`, and on this package and `build_runner` for the code they generate:

```yaml
dependencies:
  darudb: ^1.0.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.0.0
```

It declares each collection as an immutable class annotated `@Collection()`, in a library with a part named after it:

```dart
// lib/user.dart
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

`dart run build_runner build` writes `lib/user.g.dart`. It holds `userSchema`, the collection's schema, with the code that writes a `User` as a record and reads one back; `UserQuery`, whose typed fields make the conditions of a query; and a `copyWith` extension on `User`. The application uses them through the `darudb` package:

```dart
import 'dart:io';

import 'package:darudb/darudb.dart';

import 'user.dart';

void main() {
  final Database db = Database.open(
    'app.darudb',
    schema: const Schema(1, [userSchema]),
  );

  final int id = db.write(
    (txn) =>
        txn.collection(userSchema).insert(const User(name: 'Ada', age: 36)),
  );

  final List<User> adults = db.read(
    (txn) => txn
        .collection(userSchema)
        .find(
          (q) => q.where(q.age.atLeast(18)).sortBy(q.age, descending: true),
        ),
  );

  stdout.writeln('Adults: ${adults.map((user) => user.name).join(', ')}');

  db.write((txn) {
    final users = txn.collection(userSchema);

    users.put(users.get(id)!.copyWith(email: 'ada@example.com'));
  });

  db.close();
}
```

A class the generator cannot store stops the build with an error that names the field, such as a field that is not `final` or a type the database does not store. The `darudb` package has a complete example that runs, in its own `example/main.dart`.
