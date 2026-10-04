import 'dart:io';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'support/versions.dart';

Matcher failsWith(String code) =>
    throwsA(isA<DaruException>().having((error) => error.code, 'code', code));

void main() {
  late Directory directory;
  late String path;

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb-dart-');
    path = '${directory.path}/app.darudb';
  });

  tearDown(() => directory.deleteSync(recursive: true));

  test('a migration renames, drops and fills fields through its function', () {
    final v1 = Database.open(path, schema: const Schema(1, [noteV1Schema]));

    v1.write((txn) {
      txn.collection(noteV1Schema).insertMany(const [
        NoteV1(title: 'a', body: 'one two three'),
        NoteV1(title: 'b', body: 'four'),
      ]);
    });
    expect(v1.schemaVersion, 1);
    v1.close();

    final seen = <Object>[];
    final v2 = Database.open(
      path,
      schema: const Schema(2, [noteSchema]),
      migrations: [
        Migration(
          2,
          renameFields: const {
            'notes': {'title': 'heading'},
          },
          run: (context) {
            final notes = context.collection(noteSchema);

            expect(context.previousVersion, 1);

            for (final key in context.previousKeys('notes')) {
              final old = context.previous('notes', key)!;
              final note = notes.get(key as int)!;

              seen.add(old['title']!);
              notes.put(
                Note(
                  id: note.id,
                  heading: note.heading,
                  words: (old['body']! as String).split(' ').length,
                ),
              );
            }
          },
        ),
      ],
    );

    expect(seen, ['a', 'b']);
    expect(v2.schemaVersion, 2);

    final notes = v2.read((txn) => txn.collection(noteSchema).find());

    expect(notes.map((note) => (note.heading, note.words)), [
      ('a', 3),
      ('b', 1),
    ]);
    v2.close();

    expect(
      () => Database.open(path, schema: const Schema(1, [noteV1Schema])),
      failsWith('SCHEMA_TOO_NEW'),
    );
  });

  test('a failing migration function leaves the file at the old schema', () {
    Database.open(path, schema: const Schema(1, [noteV1Schema]))
      ..write(
        (txn) => txn
            .collection(noteV1Schema)
            .insert(const NoteV1(title: 'a', body: 'b')),
      )
      ..close();

    expect(
      () => Database.open(
        path,
        schema: const Schema(2, [noteSchema]),
        migrations: [
          Migration(
            2,
            renameFields: const {
              'notes': {'title': 'heading'},
            },
            run: (context) => throw StateError('no'),
          ),
        ],
      ),
      throwsStateError,
    );

    final v1 = Database.open(path, schema: const Schema(1, [noteV1Schema]));

    expect(v1.read((txn) => txn.collection(noteV1Schema).get(1))!.body, 'b');
    v1.close();
  });

  test('a password and a key encrypt the file, and open it again', () {
    final db = Database.open(
      path,
      schema: const Schema(1, [noteV1Schema]),
      password: 'correct horse',
      passwordHashing: const PasswordHashing(
        memoryKib: 64,
        iterations: 1,
        parallelism: 1,
      ),
    );

    expect(db.isEncrypted, isTrue);
    db.write(
      (txn) => txn
          .collection(noteV1Schema)
          .insert(const NoteV1(title: 't', body: 'secret')),
    );
    db.close();

    expect(
      () => Database.open(path, schema: const Schema(1, [noteV1Schema])),
      failsWith('KEY_REQUIRED'),
    );
    expect(
      () => Database.open(
        path,
        schema: const Schema(1, [noteV1Schema]),
        password: 'wrong',
      ),
      failsWith('WRONG_KEY'),
    );

    final reopened = Database.open(
      path,
      schema: const Schema(1, [noteV1Schema]),
      password: 'correct horse',
    );
    final key = Uint8List.fromList(List.generate(32, (n) => n));

    reopened.setKey(key);
    reopened.close();

    final withKey = Database.open(
      path,
      schema: const Schema(1, [noteV1Schema]),
      key: key,
    );

    expect(
      withKey.read((txn) => txn.collection(noteV1Schema).get(1))!.body,
      'secret',
    );
    withKey.close();
  });

  test('a key and a password together are refused', () async {
    final key = Uint8List(32);
    const schema = Schema(1, [noteV1Schema]);

    expect(
      () => Database.open(path, schema: schema, key: key, password: 'p'),
      failsWith('INVALID_ARGUMENT'),
    );
    await expectLater(
      Database.openAsync(path, schema: schema, key: key, password: 'p'),
      failsWith('INVALID_ARGUMENT'),
    );
    expect(
      () => Database.salvage(path, '$path.rescued', key: key, password: 'p'),
      failsWith('INVALID_ARGUMENT'),
    );
    expect(File(path).existsSync(), isFalse, reason: 'refused before it opens');
  });

  test('a transaction function that returns a Future is refused', () {
    final db = Database.open(path, schema: const Schema(1, [noteV1Schema]));

    expect(
      () => db.write((txn) async => txn.collection(noteV1Schema).count()),
      failsWith('INVALID_ARGUMENT'),
    );
    expect(db.read((txn) => txn.collection(noteV1Schema).count()), 0);
    db.close();
  });

  test('the engine and format versions are known', () {
    expect(engineVersion, isNotEmpty);
    expect(formatVersion, greaterThan(0));
  });
}
