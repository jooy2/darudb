import 'dart:io';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'support/hand_written.dart';

void main() {
  late Directory directory;
  late Database db;

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb-dart-');
    db = Database.open(
      '${directory.path}/app.darudb',
      schema: const Schema(1, [userSchema]),
    );
  });

  tearDown(() {
    db.close();
    directory.deleteSync(recursive: true);
  });

  test('objects read back as they were written', () {
    final ada = User(
      name: 'Ada',
      email: 'ada@example.com',
      age: 36,
      tags: const ['math', 'engines'],
      photo: Uint8List.fromList([0, 1, 255]),
      address: const Address(city: 'London'),
    );
    final id = db.write((txn) => txn.collection(userSchema).insert(ada));

    expect(id, 1);

    final read = db.read((txn) => txn.collection(userSchema).get(id))!;

    expect(read.id, 1);
    expect(read.name, 'Ada');
    expect(read.email, 'ada@example.com');
    expect(read.age, 36);
    expect(read.score, 0.5);
    expect(read.tags, ['math', 'engines']);
    expect(read.photo, [0, 1, 255]);
    expect(read.address, const Address(city: 'London'));
    expect(read.friend, isNull);
    expect(db.read((txn) => txn.collection(userSchema).get(2)), isNull);
  });

  test('queries filter, sort and limit', () {
    db.write((txn) {
      txn.collection(userSchema).insertMany([
        for (var n = 0; n < 50; n++)
          User(name: 'person $n', age: n % 7, tags: [if (n.isEven) 'even']),
      ]);
    });

    db.read((txn) {
      final users = txn.collection(userSchema);

      expect(users.count(), 50);
      expect(users.count((q) => q.where(q.age.equals(3))), 7);

      final sorted = users.find(
        (q) => q
            .where(q.age.between(2, 4) & ~q.name.endsWith('9'))
            .sortBy(q.age, descending: true)
            .sortBy(q.name)
            .limit(5),
      );

      expect(sorted.map((user) => user.age), [4, 4, 4, 4, 4]);
      expect(sorted.first.name, 'person 11');
      expect(
        users.count((q) => q.where(q.tags.contains('even') | q.age.isIn([1]))),
        25 + 4,
      );
      expect(
        users.findOne((q) => q.where(q.name.startsWith('person 4')))!.id,
        5,
      );
      expect(users.findText(r'age == $0 LIMIT 2', [6]).length, 2);
      expect(users.countText(r'age >= $0', [5]), 14);

      final prepared = db.prepare(userSchema, r'name == $0');

      expect(users.findOnePrepared(prepared, ['person 7'])!.age, 0);
    });
  });

  test('put replaces, update sets fields and delete removes', () {
    final id = db.write(
      (txn) => txn.collection(userSchema).insert(const User(name: 'Ada')),
    );

    db.write((txn) {
      final users = txn.collection(userSchema);
      final ada = users.get(id)!;

      users.put(ada.copyWith(age: 37));
      expect(users.get(id)!.age, 37);
      expect(
        users.update(id, (q) => [q.age.set(38), q.email.set('a@b.c')]),
        isTrue,
      );
      expect(users.get(id)!.email, 'a@b.c');
      expect(users.update(99, (q) => [q.age.set(1)]), isFalse);
    });

    expect(
      db.read(
        (txn) =>
            txn.collection(userSchema).count((q) => q.where(q.age.equals(38))),
      ),
      1,
    );
    expect(db.write((txn) => txn.collection(userSchema).delete(id)), isTrue);
    expect(db.read((txn) => txn.collection(userSchema).get(id)), isNull);
  });

  test(
    'a refused write throws the engine code and the transaction goes on',
    () {
      db.write((txn) {
        final users = txn.collection(userSchema);

        users.insert(const User(name: 'Ada', email: 'a@x'));
        expect(
          () => users.insert(const User(name: 'Bob', email: 'a@x')),
          throwsA(
            isA<DaruException>().having((e) => e.code, 'code', 'DUPLICATE_KEY'),
          ),
        );
        users.insert(const User(name: 'Bob', email: 'b@x'));
      });

      expect(db.read((txn) => txn.collection(userSchema).count()), 2);
    },
  );

  test('a function that throws aborts its write', () {
    expect(
      () => db.write((txn) {
        txn.collection(userSchema).insert(const User(name: 'Ada'));
        throw StateError('stop');
      }),
      throwsStateError,
    );
    expect(db.read((txn) => txn.collection(userSchema).count()), 0);
  });

  test('links and embedded objects are followed by queries', () {
    db.write((txn) {
      final users = txn.collection(userSchema);
      final ada = users.insert(
        const User(
          name: 'Ada',
          address: Address(city: 'London', zip: 'N1'),
        ),
      );

      users.insert(User(name: 'Grace', friend: Link<User>(ada)));
    });

    db.read((txn) {
      final users = txn.collection(userSchema);

      expect(
        users.findOne((q) => q.where(q.friend.name.equals('Ada')))!.name,
        'Grace',
      );
      expect(
        users
            .findOne((q) => q.where(q.address.city.equals('London')))!
            .address!
            .zip,
        'N1',
      );
      expect(users.findOne((q) => q.where(q.friend.equals(1)))!.name, 'Grace');
    });
  });

  test('a closed database refuses calls with CLOSED', () {
    db.close();
    expect(
      () => db.read((txn) => txn.collection(userSchema).count()),
      throwsA(isA<DaruException>().having((e) => e.code, 'code', 'CLOSED')),
    );
  });
}
