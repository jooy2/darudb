import 'dart:io';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'support/hand_written.dart';

void main() {
  late Directory directory;
  late String path;

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb-dart-');
    path = '${directory.path}/app.darudb';
  });

  tearDown(() => directory.deleteSync(recursive: true));

  Database filled() {
    final db = Database.open(path, schema: const Schema(1, [userSchema]));

    db.write((txn) {
      txn.collection(userSchema).insertMany([
        for (var n = 0; n < 2000; n++)
          User(name: 'person $n', email: '$n@example.com', age: n % 50),
      ]);
    });

    return db;
  }

  test('the integrity check finds a sound file sound', () async {
    final db = filled();
    final report = db.check();

    expect(report.ok, isTrue);
    expect(report.problems, isEmpty);
    expect(report.objectsChecked, 2000);
    expect(report.pagesChecked, greaterThan(0));
    expect((await db.checkAsync()).commitId, report.commitId);
    db.close();
  });

  test('a backup opens with the same objects', () async {
    final db = filled();
    final report = db.backup('${directory.path}/copy.darudb');

    expect(report.bytes, greaterThan(0));
    expect(
      () => db.backup('${directory.path}/copy.darudb'),
      throwsA(
        isA<DaruException>().having((e) => e.code, 'code', 'INVALID_ARGUMENT'),
      ),
    );
    await db.backupAsync('${directory.path}/second.darudb');
    db.close();

    final copy = Database.open(
      '${directory.path}/second.darudb',
      schema: const Schema(1, [userSchema]),
    );

    expect(copy.read((txn) => txn.collection(userSchema).count()), 2000);
    copy.close();
  });

  test('a backup under a new key or password opens only with it', () async {
    final key = Uint8List(32)..fillRange(0, 32, 3);
    final newKey = Uint8List(32)..fillRange(0, 32, 5);
    const schema = Schema(1, [userSchema]);
    final db = Database.open(path, schema: schema, key: key);

    db.write((txn) {
      txn
          .collection(userSchema)
          .insert(const User(name: 'one', email: 'one@example.com', age: 1));
    });
    db.backup(
      '${directory.path}/password.darudb',
      password: 'a new password',
      passwordHashing: const PasswordHashing(
        memoryKib: 8192,
        iterations: 1,
        parallelism: 1,
      ),
    );
    await db.backupAsync('${directory.path}/key.darudb', key: newKey);
    for (final refused in [
      () => db.backup('${directory.path}/empty.darudb', password: ''),
      () => db.backup(
        '${directory.path}/both.darudb',
        key: newKey,
        password: 'x',
      ),
      () => db.backup('${directory.path}/short.darudb', key: Uint8List(16)),
    ]) {
      expect(
        refused,
        throwsA(
          isA<DaruException>().having(
            (e) => e.code,
            'code',
            'INVALID_ARGUMENT',
          ),
        ),
      );
    }
    db.close();

    final plain = Database.open(
      '${directory.path}/plain.darudb',
      schema: schema,
    );

    plain.write((txn) {
      txn
          .collection(userSchema)
          .insert(const User(name: 'one', email: 'one@example.com', age: 1));
    });
    plain.backup('${directory.path}/encrypted.darudb', key: newKey);
    plain.close();

    expect(newKey, everyElement(5), reason: 'the caller wipes its own key');

    for (final (name, copy) in [
      (
        'password.darudb',
        () => Database.open(
          '${directory.path}/password.darudb',
          schema: schema,
          password: 'a new password',
        ),
      ),
      (
        'key.darudb',
        () => Database.open(
          '${directory.path}/key.darudb',
          schema: schema,
          key: newKey,
        ),
      ),
      (
        'encrypted.darudb',
        () => Database.open(
          '${directory.path}/encrypted.darudb',
          schema: schema,
          key: newKey,
        ),
      ),
    ]) {
      final opened = copy();

      expect(opened.isEncrypted, isTrue, reason: name);
      expect(
        opened.read((txn) => txn.collection(userSchema).count()),
        1,
        reason: name,
      );
      opened.close();
    }

    expect(
      () => Database.open(
        '${directory.path}/key.darudb',
        schema: schema,
        key: key,
      ),
      throwsA(isA<DaruException>().having((e) => e.code, 'code', 'WRONG_KEY')),
    );
    expect(
      () => Database.open('${directory.path}/encrypted.darudb', schema: schema),
      throwsA(
        isA<DaruException>().having((e) => e.code, 'code', 'KEY_REQUIRED'),
      ),
    );
  });

  test('compaction makes the file smaller after deletes', () async {
    final db = filled();

    db.write((txn) {
      final users = txn.collection(userSchema);

      for (var id = 1; id <= 1900; id++) {
        users.delete(id);
      }
    });

    final report = db.compact();

    expect(report.bytesAfter, lessThanOrEqualTo(report.bytesBefore));
    expect(
      (await db.compactAsync()).bytesAfter,
      lessThanOrEqualTo(report.bytesAfter),
    );
    expect(db.read((txn) => txn.collection(userSchema).count()), 100);
    expect(db.check().ok, isTrue);
    db.close();
  });

  test('salvage rescues a damaged file into a new one', () async {
    filled().close();

    final damaged = '${directory.path}/damaged.darudb';
    final bytes = File(path).readAsBytesSync();

    // Some page in the middle of the file, which holds objects.
    final page = (bytes.length ~/ 4096) ~/ 2;

    bytes.fillRange(page * 4096 + 64, page * 4096 + 512, 0xA5);
    File(damaged).writeAsBytesSync(bytes);

    final report = Database.salvage(
      damaged,
      '${directory.path}/rescued.darudb',
    );

    expect(report.pagesScanned, greaterThan(0));

    final rescued = Database.open(
      '${directory.path}/rescued.darudb',
      schema: const Schema(1, [userSchema]),
    );

    expect(rescued.check().ok, isTrue);
    expect(
      rescued.read((txn) => txn.collection(userSchema).count()) +
          report.objectsDropped,
      greaterThan(0),
    );
    rescued.close();

    final whole = await Database.salvageAsync(
      path,
      '${directory.path}/whole.darudb',
    );

    expect(whole.whole, isTrue);
    expect(whole.objectsDropped, 0);
  });

  test('salvage of an encrypted file needs its key', () {
    final key = Uint8List.fromList(List.filled(32, 7));

    Database.open(path, schema: const Schema(1, [userSchema]), key: key)
      ..write((txn) => txn.collection(userSchema).insert(const User(name: 'a')))
      ..close();

    expect(
      () => Database.salvage(path, '${directory.path}/a.darudb'),
      throwsA(
        isA<DaruException>().having((e) => e.code, 'code', 'KEY_REQUIRED'),
      ),
    );

    final report = Database.salvage(
      path,
      '${directory.path}/b.darudb',
      key: key,
    );

    expect(report.whole, isTrue);
  });
}
