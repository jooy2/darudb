// Stores a few users in a new database, finds them by an indexed field, in
// code and as text, changes one, and opens the file again.
//
// `User` is an annotated class, and `dart run build_runner build` writes
// `main.g.dart` beside this file, with `userSchema` and the query builder the
// queries use. Run it with `dart run example/main.dart`.
import 'dart:io';

import 'package:darudb/darudb.dart';

part 'main.g.dart';

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

const Schema schema = Schema(1, [userSchema]);

void main() {
  final Directory dir = Directory.systemTemp.createTempSync('darudb_example');
  final String path = '${dir.path}/app.darudb';

  try {
    final Database db = Database.open(path, schema: schema);

    // A write commits when its function returns, and aborts if it throws.
    final List<int> ids = db.write(
      (txn) => txn.collection(userSchema).insertMany(const [
        User(name: 'Ada', email: 'ada@example.com', age: 36),
        User(name: 'Alan', age: 41),
        User(name: 'Grace', age: 17),
      ]),
    );

    // The index on `age` finds the adults without reading every user.
    final List<User> adults = db.read(
      (txn) => txn
          .collection(userSchema)
          .find(
            (q) => q.where(q.age.atLeast(18)).sortBy(q.age, descending: true),
          ),
    );

    stdout.writeln('Adults: ${adults.map((user) => user.name).join(', ')}');

    // The same query in the query language, with its value as a parameter.
    final List<User> same = db.read(
      (txn) => txn.collection(userSchema).findText(
        r'age >= $0 SORT BY age DESC',
        [18],
      ),
    );

    stdout.writeln('Same query as text: ${same.length} users');

    // `update` changes the fields it is given and keeps the rest.
    db.write((txn) {
      txn.collection(userSchema).update(ids[2], (q) => [q.age.set(18)]);
    });

    db.close();

    // Everything committed is in the file when it opens again.
    final Database reopened = Database.open(path, schema: schema);
    final int count = reopened.read(
      (txn) =>
          txn.collection(userSchema).count((q) => q.where(q.age.atLeast(18))),
    );

    stdout.writeln('Adults after reopening: $count');
    reopened.close();
  } on DaruException catch (error) {
    // Every failure carries the engine's code, the same in every language.
    stderr.writeln('DaruDB failed with ${error.code}: ${error.message}');
    exitCode = 1;
  } finally {
    dir.deleteSync(recursive: true);
  }
}
