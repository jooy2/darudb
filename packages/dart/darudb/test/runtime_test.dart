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

  test('linkedFrom tests what links to an object', () {
    db.write((txn) {
      txn.collection(userSchema).insertMany([
        const User(name: 'Ada', age: 36),
        const User(name: 'Grace', age: 85, friend: Link(1)),
        const User(name: 'Alan', age: 41, friend: Link(1)),
        const User(name: 'Edsger', age: 72, friend: Link(2)),
      ]);
    });

    db.read((txn) {
      final users = txn.collection(userSchema);
      List<String> names(QueryBuilder<User> Function(UserQuery q) query) => [
        for (final user in users.find(query)) user.name,
      ];

      expect(names((q) => q.where(q.linkedFrom(userSchema, (u) => u.friend))), [
        'Ada',
        'Grace',
      ]);
      expect(
        names(
          (q) => q.where(
            q.linkedFrom(
              userSchema,
              (u) => u.friend,
              (u) => u.age.greaterThan(80),
            ),
          ),
        ),
        ['Ada'],
      );
      expect(
        names((q) => q.where(~q.linkedFrom(userSchema, (u) => u.friend))),
        ['Alan', 'Edsger'],
      );
      expect(users.countText(r'ANY users.friend (age < $0)', [50]), 1);
    });
  });

  test('tests that ignore case', () {
    db.write((txn) {
      txn.collection(userSchema).insertMany(const [
        User(name: 'Ada', email: 'ada@example.com', tags: ['Math']),
        User(name: 'GRACE', tags: ['navy']),
        User(
          name: 'σοφία',
          address: Address(city: 'ATHENS'),
        ),
      ]);
    });

    db.read((txn) {
      final users = txn.collection(userSchema);
      List<String> names(QueryBuilder<User> Function(UserQuery q) query) => [
        for (final user in users.find(query)) user.name,
      ];

      expect(names((q) => q.where(q.name.equals('ada'))), isEmpty);
      expect(names((q) => q.where(q.name.equals('ada', ignoreCase: true))), [
        'Ada',
      ]);
      expect(
        names((q) => q.where(q.name.isIn(['grace', 'X'], ignoreCase: true))),
        ['GRACE'],
      );
      expect(
        names((q) => q.where(q.name.startsWith('ΣΟΦ', ignoreCase: true))),
        ['σοφία'],
      );
      expect(names((q) => q.where(q.tags.contains('MATH', ignoreCase: true))), [
        'Ada',
      ]);
      expect(
        names(
          (q) => q.where(
            (q.name.endsWith('CE') | q.address.city.equals('athens'))
                .ignoreCase(),
          ),
        ),
        ['GRACE', 'σοφία'],
      );
      expect(users.countText('email == "ADA@EXAMPLE.COM" IGNORE CASE'), 1);
      expect(
        () => users.find(
          (q) => q.where((q.age.equals(3) & q.name.equals('x')).ignoreCase()),
        ),
        throwsA(
          isA<DaruException>().having((e) => e.code, 'code', 'INVALID_QUERY'),
        ),
      );
    });
  });

  test('aggregates work out one value from a field', () {
    db.write((txn) {
      txn.collection(userSchema).insertMany([
        for (var n = 0; n < 10; n++)
          User(
            name: 'person $n',
            age: n,
            score: n / 4,
            address: n.isEven ? Address(city: 'city ${n % 3}') : null,
          ),
      ]);
    });

    db.read((txn) {
      final users = txn.collection(userSchema);

      final int total = users.sum((q) => q.age);
      final double scores = users.sum((q) => q.score);

      expect(total, 45);
      expect(scores, 11.25);
      expect(users.min((q) => q.age), 0);
      expect(users.max((q) => q.name), 'person 9');
      expect(users.avg((q) => q.age), 4.5);
      expect(users.max((q) => q.address.city), 'city 2');
      expect(users.sum((q) => q.age, (q) => q.where(q.age.atLeast(5))), 35);
      expect(
        users.sum((q) => q.age, (q) => q.sortBy(q.age).limit(3)),
        3,
        reason: 'after the sort and the limit',
      );
      expect(users.sum((q) => q.age, (q) => q.where(q.age.greaterThan(99))), 0);
      expect(
        users.avg((q) => q.age, (q) => q.where(q.age.greaterThan(99))),
        isNull,
      );
      expect(
        users.min((q) => q.name, (q) => q.where(q.age.greaterThan(99))),
        isNull,
      );

      expect(users.sumText((q) => q.age, r'age < $0', [3]), 3);
      expect(users.maxText((q) => q.score, 'age < 4'), 0.75);
      expect(users.avgText((q) => q.age, 'age >= 8'), 8.5);

      final older = db.prepare(userSchema, r'age > $0');

      expect(users.sumPrepared((q) => q.age, older, [7]), 17);
      expect(users.minPrepared((q) => q.name, older, [7]), 'person 8');
      expect(users.avgPrepared((q) => q.age, older, [100]), isNull);
    });

    db.write((txn) {
      final users = txn.collection(userSchema);

      users.insert(const User(name: 'late', age: 100));
      expect(
        users.max((q) => q.age),
        100,
        reason: "with the transaction's changes",
      );
    });
  });

  test('deleteWhere and updateWhere change what a query finds', () {
    db.write((txn) {
      txn.collection(userSchema).insertMany([
        for (var n = 0; n < 6; n++)
          User(name: 'person $n', age: n, email: 'p$n@example.com'),
      ]);
    });

    db.write((txn) {
      final users = txn.collection(userSchema);

      expect(
        users.updateWhere(
          (q) => [q.score.set(9.5)],
          (q) => q.where(q.age.atLeast(4)),
        ),
        2,
      );
      expect(users.get(5)!.score, 9.5);
      expect(users.get(1)!.score, 0.5);
      expect(
        users.updateWhereText((q) => [q.age.set(10)], r'age < $0', [1]),
        1,
      );
      expect(users.get(1)!.age, 10);

      // A unique email cannot go to two objects: the first is put back.
      expect(
        () => users.updateWhere(
          (q) => [q.email.set('same@example.com'), q.age.set(0)],
          (q) => q,
        ),
        throwsA(
          isA<DaruException>().having((e) => e.code, 'code', 'DUPLICATE_KEY'),
        ),
      );
      expect(users.get(1)!.email, 'p0@example.com');
      expect(users.get(1)!.age, 10);

      final older = db.prepare(userSchema, r'age > $0');

      expect(users.deleteWherePrepared(older, [3]), 3);
      expect(users.deleteWhereText('age == 2'), 1);
      expect(users.deleteWhere((q) => q.where(q.age.lessThan(2))), 1);
      expect(users.count(), 1);
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

  test('a transaction used after its function fails with CLOSED', () {
    late ReadCollection<User, UserQuery, int> kept;

    db.read((txn) => kept = txn.collection(userSchema));
    expect(
      () => kept.count(),
      throwsA(isA<DaruException>().having((e) => e.code, 'code', 'CLOSED')),
    );
  });

  test('a write inside a write on the same file fails at once', () {
    expect(
      () => db.write((txn) => db.write((inner) => 1)),
      throwsA(
        isA<DaruException>().having((e) => e.code, 'code', 'INVALID_ARGUMENT'),
      ),
    );
    expect(
      () => db.write((txn) => db.sync()),
      throwsA(
        isA<DaruException>().having((e) => e.code, 'code', 'INVALID_ARGUMENT'),
      ),
    );
    // The refused calls left the file usable.
    db.write((txn) => txn.collection(userSchema).insert(const User(name: 'a')));
    expect(db.read((txn) => txn.collection(userSchema).count()), 1);
  });

  test('a closed database refuses calls with CLOSED', () {
    db.close();
    expect(
      () => db.read((txn) => txn.collection(userSchema).count()),
      throwsA(isA<DaruException>().having((e) => e.code, 'code', 'CLOSED')),
    );
  });
}
