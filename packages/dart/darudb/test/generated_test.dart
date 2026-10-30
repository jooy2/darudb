import 'dart:io';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'models.dart';

void main() {
  late Directory directory;
  late Database db;

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb-dart-');
    db = Database.open(
      '${directory.path}/app.darudb',
      schema: const Schema(1, [personSchema, postSchema]),
    );
  });

  tearDown(() {
    db.close();
    directory.deleteSync(recursive: true);
  });

  test('generated classes write and read every kind of field', () {
    final ada = Person(
      name: 'Ada',
      email: 'ada@example.com',
      age: 36,
      tags: const ['math'],
      photo: Uint8List.fromList([1, 2, 3]),
      home: const Address(city: 'London', zip: 'N1', point: [-0.12, 51.5]),
    );

    db.write((txn) {
      final people = txn.collection(personSchema);
      final id = people.insert(ada);

      people.insert(Person(name: 'Grace', friend: Link<Person>(id)));
      txn
          .collection(postSchema)
          .insert(
            Post(
              'engines',
              author: Link<Person>(id),
              readers: [Link<Person>(2)],
            ),
          );
    });

    db.read((txn) {
      final people = txn.collection(personSchema);
      final read = people.get(1)!;

      expect(read.id, 1);
      expect(read.email, 'ada@example.com');
      expect(read.score, 0.5);
      expect(read.active, isTrue);
      expect(read.photo, [1, 2, 3]);
      expect(read.home!.zip, 'N1');
      expect(read.home!.point, [-0.12, 51.5]);
      expect(people.get(2)!.friend, Link<Person>(1));
      expect(
        people.findOne((q) => q.where(q.home.zip.equals('N1')))!.name,
        'Ada',
      );
      expect(
        people.findOne((q) => q.where(q.friend.name.equals('Ada')))!.name,
        'Grace',
      );

      final post = txn.collection(postSchema).get('engines')!;

      expect(post.author.key, 1);
      expect(post.readers, [Link<Person>(2)]);
      expect(
        txn
            .collection(postSchema)
            .count((q) => q.where(q.author.name.startsWith('A'))),
        1,
      );
    });
  });

  test('a migration reads a list of links as the keys they hold', () {
    db.write((txn) {
      final people = txn.collection(personSchema);
      final ada = people.insert(const Person(name: 'Ada'));
      final grace = people.insert(const Person(name: 'Grace'));

      txn
          .collection(postSchema)
          .insert(
            Post(
              'engines',
              author: Link<Person>(ada),
              readers: [Link<Person>(ada), Link<Person>(grace)],
            ),
          );
    });
    db.close();

    Object? readers;

    db = Database.open(
      '${directory.path}/app.darudb',
      schema: const Schema(2, [personSchema, postSchema]),
      migrations: [
        Migration(
          2,
          run: (context) =>
              readers = context.previous('posts', 'engines')!['readers'],
        ),
      ],
    );

    expect(readers, [1, 2]);
  });

  test('copyWith and put replace an object, and update sets one field', () {
    final id = db.write(
      (txn) => txn.collection(personSchema).insert(const Person(name: 'Ada')),
    );

    db.write((txn) {
      final people = txn.collection(personSchema);

      people.put(people.get(id)!.copyWith(age: 37, tags: ['x']));
      people.update(id, (q) => [q.email.set('ada@example.com')]);
    });

    final ada = db.read((txn) => txn.collection(personSchema).get(id))!;

    expect(ada.age, 37);
    expect(ada.tags, ['x']);
    expect(ada.email, 'ada@example.com');
  });

  group('update replaces an embedded object whole', () {
    late Database places;
    late int id;

    // A file the first version made, opened with the second, whose embedded
    // object lists its fields in another order than the file keeps them.
    setUp(() {
      final path = '${directory.path}/places.darudb';
      final first = Database.open(
        path,
        schema: const Schema(1, [placeV1Schema]),
      );

      id = first.write(
        (txn) => txn
            .collection(placeV1Schema)
            .insert(const PlaceV1(spot: SpotV1(city: 'Seoul'))),
      );
      first.close();
      places = Database.open(path, schema: const Schema(2, [placeSchema]));
    });

    tearDown(() => places.close());

    Spot? spot() =>
        places.read((txn) => txn.collection(placeSchema).get(id))!.spot;

    test('with its fields where the file keeps them', () {
      places.write((txn) {
        final updated = txn
            .collection(placeSchema)
            .update(
              id,
              (q) => [q.spot.set(const Spot(zip: 'N1', city: 'London'))],
            );

        expect(updated, isTrue);
      });

      expect(spot()!.city, 'London');
      expect(spot()!.zip, 'N1');
      expect(
        places.read(
          (txn) => txn
              .collection(placeSchema)
              .count((q) => q.where(q.spot.city.equals('London'))),
        ),
        1,
      );
      expect(
        places.read(
          (txn) =>
              txn.collection(placeSchema).countText(r'spot.zip == $0', ['N1']),
        ),
        1,
      );
    });

    test('or makes it null', () {
      places.write(
        (txn) =>
            txn.collection(placeSchema).update(id, (q) => [q.spot.set(null)]),
      );

      expect(spot(), isNull);
    });

    test('through the Future API', () async {
      await places.writeAsync(
        (txn) => txn
            .collection(placeSchema)
            .update(id, (q) => [q.spot.set(const Spot(city: 'Busan'))]),
      );

      expect(spot()!.city, 'Busan');
      expect(spot()!.zip, isNull);
    });

    test('but not a field inside it', () {
      expect(
        () => places.write(
          (txn) => txn
              .collection(placeSchema)
              .update(id, (q) => [q.spot.city.set('Daegu')]),
        ),
        throwsA(
          isA<DaruException>().having(
            (error) => error.code,
            'code',
            'INVALID_ARGUMENT',
          ),
        ),
      );
    });
  });

  test('a declared key that is taken is DUPLICATE_KEY', () {
    db.write((txn) {
      final people = txn.collection(personSchema);
      final posts = txn.collection(postSchema);
      final id = people.insert(const Person(name: 'Ada'));

      posts.insert(Post('a', author: Link<Person>(id)));
      expect(
        () => posts.insert(Post('a', author: Link<Person>(id))),
        throwsA(
          isA<DaruException>().having((e) => e.code, 'code', 'DUPLICATE_KEY'),
        ),
      );
    });
  });

  test('a DateTime field is a date, kept to the microsecond in UTC', () {
    final events = Database.open(
      '${directory.path}/events.darudb',
      schema: const Schema(1, [eventSchema]),
    );

    addTearDown(events.close);

    final launch = DateTime.utc(2026, 10, 30, 12, 0, 0, 123, 456);
    final local = DateTime.fromMicrosecondsSinceEpoch(-1);
    final ancient = DateTime.utc(1, 1, 1);

    events.write((txn) {
      txn.collection(eventSchema)
        ..insert(Event(at: launch, seen: [launch, local]))
        ..insert(Event(at: local, ends: launch))
        ..insert(Event(at: ancient));
    });
    events.read((txn) {
      final all = txn.collection(eventSchema);
      final first = all.get(1)!;

      expect(first.at, launch);
      expect(first.at.isUtc, isTrue);
      expect(first.seen, [launch, local.toUtc()]);
      expect(all.get(2)!.at.microsecondsSinceEpoch, -1);
      expect(all.get(2)!.ends, launch);
      expect(
        [
          for (final event in all.find(
            (q) => q.where(q.at.lessThan(DateTime.utc(1970))).sortBy(q.at),
          ))
            event.id,
        ],
        [3, 2],
      );
      expect(
        [for (final event in all.findText('at >= DATE "2026-10-30"')) event.id],
        [1],
      );
      expect(
        [
          for (final event in all.find((q) => q.where(q.seen.contains(local))))
            event.id,
        ],
        [1],
      );
      expect(all.min((q) => q.at), ancient);
      expect(all.max((q) => q.ends), launch);
    });
  });

  test('a List of an embedded class is a list of embedded objects', () {
    final trips = Database.open(
      '${directory.path}/trips.darudb',
      schema: const Schema(1, [tripSchema]),
    );

    addTearDown(trips.close);

    const coast = [Stop(city: 'Busan', minutes: 30), Stop(city: 'Ulsan')];

    trips.write((txn) {
      txn.collection(tripSchema)
        ..insert(const Trip(name: 'coast', stops: coast))
        ..insert(const Trip(name: 'none'));
    });

    List<int?> ids(List<Trip> found) => [for (final trip in found) trip.id];

    trips.read((txn) {
      final all = txn.collection(tripSchema);
      final first = all.get(1)!;

      expect(
        [for (final stop in first.stops) (stop.city, stop.minutes)],
        [('Busan', 30), ('Ulsan', null)],
      );
      expect(all.get(2)!.stops, isEmpty);
      expect(ids(all.find((q) => q.where(q.stops.city.equals('Ulsan')))), [1]);
      expect(ids(all.find((q) => q.where(q.stops.minutes.greaterThan(10)))), [
        1,
      ]);
      expect(ids(all.findText('stops.mins IS NULL')), [1]);
    });
    trips.write((txn) {
      txn
          .collection(tripSchema)
          .update(
            2,
            (q) => [
              q.stops.set(const [Stop(city: 'Jeju')]),
            ],
          );
    });
    expect(
      trips.read(
        (txn) => [
          for (final stop in txn.collection(tripSchema).get(2)!.stops)
            stop.city,
        ],
      ),
      ['Jeju'],
    );
  });

  test('@IndexOn keeps an index on several fields, and on embedded ones', () {
    final residents = Database.open(
      '${directory.path}/residents.darudb',
      schema: const Schema(1, [residentSchema]),
    );

    addTearDown(residents.close);

    residents.write((txn) {
      final all = txn.collection(residentSchema)
        ..insert(
          const Resident(
            last: 'Kim',
            first: 'Ada',
            age: 36,
            home: Hometown(city: 'Seoul'),
          ),
        )
        ..insert(
          const Resident(
            last: 'Kim',
            first: 'Bo',
            age: 20,
            home: Hometown(city: 'Seoul'),
          ),
        )
        ..insert(const Resident(last: 'Kim', age: 50))
        ..insert(const Resident(last: 'Kim', age: 51));

      expect(
        () => all.insert(const Resident(last: 'Kim', first: 'Ada')),
        throwsA(
          isA<DaruException>().having((e) => e.code, 'code', 'DUPLICATE_KEY'),
        ),
      );
    });
    residents.read((txn) {
      final all = txn.collection(residentSchema);

      expect(
        [
          for (final resident in all.find(
            (q) => q.where(q.home.city.equals('Seoul')).sortBy(q.age),
          ))
            resident.first,
        ],
        ['Bo', 'Ada'],
      );
      expect(
        all.count((q) => q.where(q.last.equals('Kim') & q.first.isNull())),
        2,
      );
    });

    final stored = residents.storedSchema!.collections.single;

    expect(
      [
        for (final index in stored.indexes)
          '${index.fields.join(',')} ${index.unique}',
      ],
      ['last,given true', 'home.town,age false'],
    );
  });
}
