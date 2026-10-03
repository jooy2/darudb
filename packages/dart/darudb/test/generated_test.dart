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
}
