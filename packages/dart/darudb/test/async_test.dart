import 'dart:async';
import 'dart:io';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'support/hand_written.dart';
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

  test(
    'openAsync fails through its Future, even for a schema the package refuses',
    () {
      late Future<Database> opening;

      expect(
        () => opening = Database.openAsync(
          path,
          schema: const Schema(1, [userSchema, userSchema]),
        ),
        returnsNormally,
      );

      return expectLater(opening, failsWith('INVALID_ARGUMENT'));
    },
  );

  test(
    'a call of an asynchronous collection fails through its Future',
    () async {
      final db = Database.open(path, schema: const Schema(1, [userSchema]));

      await db.writeAsync((txn) async {
        final users = txn.collection(userSchema);
        late Future<bool> updating;

        expect(
          () => updating = users.update(1, (q) => [q.friend.name.set('x')]),
          returnsNormally,
        );
        await expectLater(updating, failsWith('INVALID_ARGUMENT'));
      });

      db.close();
    },
  );

  test('the Future API writes, reads, queries and closes', () async {
    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );

    final ids = await db.writeAsync(
      (txn) => txn.collection(userSchema).insertMany([
        for (var n = 0; n < 20; n++) User(name: 'person $n', age: n % 4),
      ]),
    );

    expect(ids, List.generate(20, (n) => n + 1));

    await db.readAsync((txn) async {
      final users = txn.collection(userSchema);

      expect((await users.get(3))!.name, 'person 2');
      expect(await users.get(99), isNull);
      expect(await users.count((q) => q.where(q.age.equals(1))), 5);
      expect(
        (await users.find((q) => q.where(q.age.atLeast(3)).limit(2))).length,
        2,
      );
      expect((await users.findOneText(r'name == $0', ['person 7']))!.id, 8);
      expect(await users.countText(r'age < $0', [2]), 10);
    });

    await db.writeAsync((txn) async {
      final users = txn.collection(userSchema);

      expect(await users.update(1, (q) => [q.age.set(40)]), isTrue);
      expect(await users.delete(2), isTrue);
      expect(await users.delete(2), isFalse);
    });

    expect(db.read((txn) => txn.collection(userSchema).get(1))!.age, 40);
    await db.syncAsync();
    await db.closeAsync();
    expect(db.isOpen, isFalse);
  });

  test('calls of one transaction run in the order they were made', () async {
    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );

    final counted = await db.writeAsync((txn) {
      final users = txn.collection(userSchema);

      // None awaited: each runs after the one before it.
      users.insert(const User(name: 'a'));
      users.insert(const User(name: 'b'));
      users.delete(1);

      return users.count();
    });

    expect(counted, 1);
    expect(db.read((txn) => txn.collection(userSchema).get(2))!.name, 'b');
    await db.closeAsync();
  });

  test('asynchronous writes on one file take turns', () async {
    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );
    final second = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );

    await Future.wait([
      for (var n = 0; n < 10; n++)
        (n.isEven ? db : second).writeAsync((txn) async {
          final users = txn.collection(userSchema);
          final before = await users.count();

          await users.insert(User(name: 'person $n'));
          expect(await users.count(), before + 1);
        }, durability: n % 3 == 0 ? Durability.deferred : Durability.sync),
    ]);

    expect(await db.readAsync((txn) => txn.collection(userSchema).count()), 10);
    await second.closeAsync();
    await db.closeAsync();
  });

  test(
    'a synchronous write while an asynchronous one runs is refused',
    () async {
      final db = await Database.openAsync(
        path,
        schema: const Schema(1, [userSchema]),
      );
      final entered = Completer<void>();
      final leave = Completer<void>();
      final writing = db.writeAsync((txn) async {
        entered.complete();
        await leave.future;
      });

      await entered.future;
      expect(
        () => db.write((txn) => txn.collection(userSchema).count()),
        failsWith('INVALID_ARGUMENT'),
      );
      expect(() => db.close(), failsWith('INVALID_ARGUMENT'));
      // A read waits for no writer.
      expect(db.read((txn) => txn.collection(userSchema).count()), 0);
      leave.complete();
      await writing;
      db.write(
        (txn) => txn.collection(userSchema).insert(const User(name: 'a')),
      );
      await db.closeAsync();
    },
  );

  test('a write inside a write on the same file is refused', () async {
    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );

    await expectLater(
      db.writeAsync((txn) => db.writeAsync((inner) => 1)),
      failsWith('INVALID_ARGUMENT'),
    );
    await expectLater(
      db.writeAsync((txn) => db.syncAsync()),
      failsWith('INVALID_ARGUMENT'),
    );
    // Another file's write is not nested.
    final other = await Database.openAsync(
      '${directory.path}/other.darudb',
      schema: const Schema(1, [userSchema]),
    );

    await db.writeAsync(
      (txn) => other.writeAsync(
        (inner) => inner.collection(userSchema).insert(const User(name: 'a')),
      ),
    );
    expect(other.read((txn) => txn.collection(userSchema).count()), 1);
    await other.closeAsync();
    await db.closeAsync();
  });

  test('a failing function or call aborts the write, with the code', () async {
    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );

    await expectLater(
      db.writeAsync((txn) async {
        await txn.collection(userSchema).insert(const User(name: 'a'));
        throw StateError('stop');
      }),
      throwsStateError,
    );
    await expectLater(
      db.writeAsync((txn) async {
        final users = txn.collection(userSchema);

        await users.insert(const User(name: 'a', email: 'x'));
        await users.insert(const User(name: 'b', email: 'x'));
      }),
      failsWith('DUPLICATE_KEY'),
    );
    expect(await db.readAsync((txn) => txn.collection(userSchema).count()), 0);
    await db.closeAsync();
  });

  test('openAsync runs an asynchronous migration function', () async {
    final v1 = Database.open(path, schema: const Schema(1, [noteV1Schema]));

    v1.write(
      (txn) => txn
          .collection(noteV1Schema)
          .insert(const NoteV1(title: 'a', body: 'one two')),
    );
    v1.close();

    final v2 = await Database.openAsync(
      path,
      schema: const Schema(2, [noteSchema]),
      migrations: [
        Migration(
          2,
          renameFields: const {
            'notes': {'title': 'heading'},
          },
          run: (context) async {
            await Future<void>.delayed(Duration.zero);

            final old = context.previous('notes', 1)!;

            context
                .collection(noteSchema)
                .put(
                  Note(
                    id: 1,
                    heading: old['title']! as String,
                    words: (old['body']! as String).split(' ').length,
                  ),
                );
          },
        ),
      ],
    );

    expect(v2.read((txn) => txn.collection(noteSchema).get(1))!.words, 2);
    await v2.closeAsync();
  });

  test('a call on a closed database fails with CLOSED', () async {
    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [userSchema]),
    );

    await db.closeAsync();
    await expectLater(
      db.readAsync((txn) => txn.collection(userSchema).count()),
      failsWith('CLOSED'),
    );
    await expectLater(db.syncAsync(), failsWith('CLOSED'));
  });
}
