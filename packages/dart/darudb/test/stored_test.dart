// A file opened without a schema: the database works with the schema the
// file holds, whose collections have maps for objects, by the names the file
// stores, and describes it through `storedSchema`.
import 'dart:io';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'models.dart'
    show Event, Resident, Stop, Trip, eventSchema, residentSchema, tripSchema;
import 'support/hand_written.dart';
import 'support/versions.dart';

Matcher failsWith(String code, [String? saying]) => throwsA(
  isA<DaruException>()
      .having((error) => error.code, 'code', code)
      .having((error) => error.message, 'message', contains(saying ?? '')),
);

void main() {
  late Directory directory;
  late String path;

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb-dart-');
    path = '${directory.path}/app.darudb';
  });

  tearDown(() => directory.deleteSync(recursive: true));

  /// Two users, written through the hand-written schema.
  void withUsers() {
    final db = Database.open(path, schema: const Schema(1, [userSchema]));

    db.write(
      (txn) => txn.collection(userSchema).insertMany([
        User(
          name: 'Ada',
          email: 'ada@example.com',
          age: 36,
          tags: const ['math'],
          photo: Uint8List.fromList([1, 2]),
          address: const Address(city: 'London'),
        ),
        const User(name: 'Grace', age: 17, friend: Link(1)),
      ]),
    );
    db.close();
  }

  test('the file\'s own schema describes its collections', () {
    withUsers();

    final db = Database.open(path);

    addTearDown(db.close);

    final schema = db.storedSchema!;
    final users = db.storedCollection('users');

    expect(schema.version, 1);
    expect(db.schemaVersion, 1);
    expect(schema.collections.single, same(users));
    expect(users.name, 'users');
    expect(users.autoKey, isTrue);
    expect(users.fields.map((field) => field.name), [
      'id',
      'name',
      'email',
      'age',
      'score',
      'tags',
      'photo',
      'address',
      'friend',
    ]);

    final fields = {for (final field in users.fields) field.name: field};

    expect(fields['email']!.unique, isTrue);
    expect(fields['email']!.optional, isTrue);
    expect(fields['age']!.index, isTrue);
    expect(fields['age']!.defaultValue, 0);
    expect(fields['score']!.kind, isA<FloatKind>());
    expect(fields['score']!.defaultValue, 0.5);
    expect(
      fields['tags']!.kind,
      isA<ListKind>().having(
        (kind) => kind.element,
        'element',
        isA<StringKind>(),
      ),
    );
    expect(
      fields['friend']!.kind,
      isA<LinkKind>().having((kind) => kind.collection, 'collection', 'users'),
    );
    expect(
      (fields['address']!.kind as ObjectKind).embedded.fields.map(
        (field) => field.name,
      ),
      ['city', 'zip'],
    );
    expect(db.storedSchema, same(schema), reason: 'the same schema every time');
  });

  test('objects are read and written as maps, with the file\'s checks', () {
    withUsers();

    final db = Database.open(path);

    addTearDown(db.close);

    final users = db.storedCollection('users');
    final ada = db.read((txn) => txn.collection(users).get(1))!;

    expect(ada['name'], 'Ada');
    expect(ada['address'], {'city': 'London', 'zip': null});
    expect(ada['photo'], Uint8List.fromList([1, 2]));
    expect(ada['friend'], isNull);
    expect(
      db.read((txn) => txn.collection(users).get(2))!['friend'],
      const Link<Object?>(1),
    );
    expect(
      db.read(
        (txn) => txn
            .collection(users)
            .find(
              (q) => q
                  .where(q.intField('age').atLeast(18))
                  .sortBy(q.stringField('name')),
            )
            .map((user) => user['name']),
      ),
      ['Ada'],
    );
    expect(
      db.read((txn) => txn.collection(users).countText('age < \$0', [18])),
      1,
    );

    db.write((txn) {
      final collection = txn.collection(users);

      // A float field takes an int, and a link a primary key, as a
      // generated class's would.
      expect(
        collection.insert({
          'name': 'Carol',
          'score': 2,
          'tags': <String>[],
          'friend': 1,
        }),
        3,
      );
      expect(
        () => collection.insert({
          'name': 'Eve',
          'email': 'ada@example.com',
          'tags': <String>[],
        }),
        failsWith('DUPLICATE_KEY'),
      );
      expect(
        () => collection.insert({'name': 'Eve', 'nick': 'e'}),
        failsWith('INVALID_ARGUMENT', 'nick'),
      );
      expect(
        () => collection.insert({'name': 'Eve', 'age': 'old'}),
        failsWith('INVALID_ARGUMENT', 'age'),
      );
      expect(
        () => collection.insert({'age': 3}),
        failsWith('INVALID_ARGUMENT'),
      );
      expect(
        collection.update(
          2,
          (q) => [
            q.intField('age').set(18),
            q.objectField('address').set({'city': 'Arlington'}),
          ],
        ),
        isTrue,
      );
    });

    final grace = db.read((txn) => txn.collection(users).get(2))!;

    expect(grace['age'], 18);
    expect(grace['address'], {'city': 'Arlington', 'zip': null});

    // A generated type reaches a collection of the file's own schema too.
    final carol = db.read((txn) => txn.collection(userSchema).get(3))!;

    expect(carol.score, 2.0);
    expect(carol.friend, const Link<User>(1));
  });

  test(
    'a file without a schema has no collections, for the engine\'s reason',
    () {
      final db = Database.open(path);

      addTearDown(db.close);
      expect(db.storedSchema, isNull);
      expect(db.schemaVersion, isNull);
      expect(
        () => db.storedCollection('users'),
        failsWith('INVALID_ARGUMENT', 'holds none'),
      );
      expect(
        () => db.read((txn) => txn.collection(userSchema)),
        failsWith('INVALID_ARGUMENT', 'holds none'),
      );

      // A schema stored after the database opened is not its own.
      Database.open(path, schema: const Schema(1, [userSchema])).close();
      expect(
        () => db.storedCollection('users'),
        failsWith('INVALID_ARGUMENT', 'open it again'),
      );
    },
  );

  test(
    'a database with a schema of its own reaches it through its constants',
    () {
      const schema = Schema(1, [userSchema]);
      final db = Database.open(path, schema: schema);

      addTearDown(db.close);
      expect(db.storedSchema, same(schema));
      expect(
        () => db.storedCollection('users'),
        failsWith('INVALID_ARGUMENT', 'schema constants'),
      );
    },
  );

  test('a migration under it is noticed', () {
    withUsers();

    final db = Database.open(path);

    addTearDown(db.close);
    Database.open(
      path,
      schema: const Schema(2, [userSchema, noteV1Schema]),
    ).close();
    expect(
      () => db.read(
        (txn) => txn.collection(db.storedCollection('users')).count(),
      ),
      failsWith('SCHEMA_MISMATCH'),
    );
    expect(db.schemaVersion, 1);
  });

  test('the asynchronous API reaches the collections by name too', () async {
    withUsers();

    final db = await Database.openAsync(path);

    addTearDown(db.close);

    final users = db.storedCollection('users');

    await db.writeAsync(
      (txn) =>
          txn.collection(users).insert({'name': 'Dave', 'tags': <String>[]}),
    );
    expect(
      await db.readAsync(
        (txn) async =>
            (await txn.collection(users).find()).map((user) => user['name']),
      ),
      ['Ada', 'Grace', 'Dave'],
    );
  });

  test('a date reads as a DateTime in UTC, and dateField queries it', () {
    final launch = DateTime.utc(2026, 10, 30, 12);
    final typed = Database.open(path, schema: const Schema(1, [eventSchema]));

    typed.write(
      (txn) => txn.collection(eventSchema)
        ..insert(Event(at: launch, seen: [launch]))
        ..insert(Event(at: DateTime.utc(1969))),
    );
    typed.close();

    final db = Database.open(path);

    addTearDown(db.close);

    final events = db.storedCollection('events');

    db.read((txn) {
      final all = txn.collection(events);

      expect(all.get(1)!['at'], launch);
      expect(all.get(1)!['seen'], [launch]);
      expect(
        all
            .find(
              (q) => q.where(q.dateField('at').lessThan(DateTime.utc(1970))),
            )
            .map((event) => event['id']),
        [2],
      );
    });
    db.write(
      (txn) => txn.collection(events).insert({'at': DateTime.utc(2000)}),
    );
    expect(
      () => db.write(
        (txn) => txn.collection(events).insert({'at': '2000-01-01'}),
      ),
      failsWith('INVALID_ARGUMENT'),
    );
  });

  test('a list of embedded objects reads as a list of maps', () {
    final typed = Database.open(path, schema: const Schema(1, [tripSchema]));

    typed.write(
      (txn) => txn
          .collection(tripSchema)
          .insert(
            const Trip(
              name: 'coast',
              stops: [Stop(city: 'Busan')],
            ),
          ),
    );
    typed.close();

    final db = Database.open(path);

    addTearDown(db.close);

    final trips = db.storedCollection('trips');

    expect(db.read((txn) => txn.collection(trips).get(1))!['stops'], [
      {'city': 'Busan', 'mins': null},
    ]);
    db.write(
      (txn) => txn.collection(trips).insert({
        'name': 'island',
        'stops': [
          {'city': 'Jeju', 'mins': 50},
        ],
      }),
    );
    expect(
      db.read(
        (txn) => txn
            .collection(trips)
            .find((q) => q.where(q.intField('stops.mins').atLeast(10)))
            .map((trip) => trip['name']),
      ),
      ['island'],
    );
    expect(
      () => db.write(
        (txn) => txn.collection(trips).insert({
          'name': 'x',
          'stops': ['Busan'],
        }),
      ),
      failsWith('INVALID_ARGUMENT'),
    );
  });

  test('an index on several fields reads back by the paths it stores', () {
    Database.open(path, schema: const Schema(1, [residentSchema]))
      ..write(
        (txn) => txn
            .collection(residentSchema)
            .insert(const Resident(last: 'Kim', first: 'Ada')),
      )
      ..close();

    final db = Database.open(path);

    addTearDown(db.close);

    final residents = db.storedCollection('residents');

    expect(
      [
        for (final index in residents.indexes)
          '${index.fields.join(',')} ${index.unique}',
      ],
      ['last,given true', 'home.town,age false'],
    );
    expect(
      () => db.write(
        (txn) =>
            txn.collection(residents).insert({'last': 'Kim', 'given': 'Ada'}),
      ),
      failsWith('DUPLICATE_KEY'),
    );
  });
}
