// The engine through the Dart package, on the device itself and apart from
// the sample's screens. What differs between operating systems is how files,
// locks and syncs behave, so these run where an app runs: two isolates
// writing to one file at once, an encrypted file through a password change,
// two backups and a salvage, a large transaction, deferred commits that a
// close makes durable, and on a phone, a file in each folder an app keeps
// files in.
//
// Run with `flutter test integration_test/engine_test.dart -d <device>`, or
// with the app's scenario by naming the folder instead.
import 'dart:io';
import 'dart:isolate';

import 'package:darudb/darudb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';

import 'package:darudb_sample/src/model.dart';

/// Whether the test runs on a phone or its simulator.
final bool _onDevice = Platform.isAndroid || Platform.isIOS;

Organization _organization(String code) => Organization(
  code: code,
  name: 'Organization $code',
  kind: 'company',
  language: 'en',
  founded: 2000,
);

Future<int> _count(Database db) => db.readAsync(
  (AsyncReadTransaction txn) => txn.collection(organizationSchema).count(),
);

/// Inserts [count] organizations whose codes begin with [prefix], one
/// deferred commit each, through the synchronous API on an isolate of its
/// own, with a handle of its own to the file.
Future<void> _insertOnIsolate(String path, String prefix, int count) =>
    Isolate.run(() {
      final Database db = Database.open(path, schema: sampleSchema);

      try {
        for (int n = 0; n < count; n++) {
          db.write(
            (WriteTransaction txn) => txn
                .collection(organizationSchema)
                .insert(_organization('$prefix-$n')),
            durability: Durability.deferred,
          );
        }
      } finally {
        db.close();
      }
    });

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  late Directory directory;

  String pathOf(String name) =>
      '${directory.path}${Platform.pathSeparator}$name';

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb_engine');
  });

  tearDown(() {
    directory.deleteSync(recursive: true);
  });

  testWidgets('two isolates write to one file at once', (
    WidgetTester tester,
  ) async {
    final String path = pathOf('shared.darudb');
    final Database db = await Database.openAsync(path, schema: sampleSchema);

    try {
      final Future<void> other = _insertOnIsolate(path, 'other', 500);

      for (int n = 0; n < 500; n++) {
        await db.writeAsync(
          (AsyncWriteTransaction txn) => txn
              .collection(organizationSchema)
              .insert(_organization('main-$n')),
          durability: Durability.deferred,
        );
      }

      await other;
      expect(await _count(db), 1000);
      expect((await db.checkAsync()).ok, isTrue);
    } finally {
      await db.closeAsync();
    }
  });

  testWidgets(
    'an encrypted file keeps its objects through a password change, two '
    'backups and a salvage',
    (WidgetTester tester) async {
      final String path = pathOf('secret.darudb');
      Database db = await Database.openAsync(
        path,
        schema: sampleSchema,
        password: 'first',
      );

      await db.writeAsync(
        (AsyncWriteTransaction txn) =>
            txn.collection(organizationSchema).insertMany(<Organization>[
              for (int n = 0; n < 100; n++) _organization('secret-$n'),
            ]),
      );
      await db.setPasswordAsync('second');
      await db.closeAsync();

      await expectLater(
        Database.openAsync(path, schema: sampleSchema, password: 'first'),
        throwsA(
          isA<DaruException>().having(
            (DaruException error) => error.code,
            'code',
            'WRONG_KEY',
          ),
        ),
      );

      db = await Database.openAsync(
        path,
        schema: sampleSchema,
        password: 'second',
      );
      expect(db.isEncrypted, isTrue);
      await db.backupAsync(pathOf('plain.darudb'), plain: true);
      await db.backupAsync(pathOf('copy.darudb'), password: 'third');
      await db.closeAsync();

      final SalvageReport salvaged = await Database.salvageAsync(
        path,
        pathOf('salvaged.darudb'),
        password: 'second',
      );

      expect(salvaged.whole, isTrue);

      // A plain backup opens with no password, the other backup with its
      // own, and the salvaged file with the old file's.
      for (final (String name, String? password) in <(String, String?)>[
        ('plain.darudb', null),
        ('copy.darudb', 'third'),
        ('salvaged.darudb', 'second'),
      ]) {
        final Database copy = await Database.openAsync(
          pathOf(name),
          schema: sampleSchema,
          password: password,
        );

        try {
          expect(copy.isEncrypted, password != null, reason: name);
          expect(await _count(copy), 100, reason: name);
        } finally {
          await copy.closeAsync();
        }
      }
    },
  );

  testWidgets('a hundred thousand objects in one transaction, half deleted, '
      'checked and compacted', (WidgetTester tester) async {
    final String path = pathOf('large.darudb');
    Database db = Database.open(path, schema: sampleSchema);

    try {
      db.write(
        (WriteTransaction txn) =>
            txn.collection(organizationSchema).insertMany(<Organization>[
              for (int n = 0; n < 100000; n++) _organization('large-$n'),
            ]),
      );
      db.write((WriteTransaction txn) {
        final collection = txn.collection(organizationSchema);

        for (int n = 0; n < 100000; n += 2) {
          collection.delete('large-$n');
        }
      });

      final CheckReport check = db.check();

      expect(check.ok, isTrue);
      expect(check.objectsChecked, 50000);

      final CompactReport compacted = db.compact();

      expect(compacted.bytesAfter, lessThan(compacted.bytesBefore));
    } finally {
      db.close();
    }

    db = Database.open(path, schema: sampleSchema);

    try {
      expect(
        db.read(
          (ReadTransaction txn) => txn.collection(organizationSchema).count(),
        ),
        50000,
      );
    } finally {
      db.close();
    }
  });

  testWidgets('deferred commits are there once the file is closed and opened '
      'again', (WidgetTester tester) async {
    final String path = pathOf('deferred.darudb');
    Database db = Database.open(path, schema: sampleSchema);

    try {
      for (int n = 0; n < 1000; n++) {
        db.write(
          (WriteTransaction txn) => txn
              .collection(organizationSchema)
              .insert(_organization('deferred-$n')),
          durability: Durability.deferred,
        );
      }
    } finally {
      db.close();
    }

    db = Database.open(path, schema: sampleSchema);

    try {
      expect(
        db.read(
          (ReadTransaction txn) => txn.collection(organizationSchema).count(),
        ),
        1000,
      );
    } finally {
      db.close();
    }
  });

  testWidgets(
    "opens a file in each folder an app keeps files in",
    (WidgetTester tester) async {
      final Map<String, Directory> folders = <String, Directory>{
        'temporary': await getTemporaryDirectory(),
        'support': await getApplicationSupportDirectory(),
        'documents': await getApplicationDocumentsDirectory(),
        'cache': await getApplicationCacheDirectory(),
        // The app's folder on shared storage, which recent versions of
        // Android serve through FUSE.
        if (Platform.isAndroid)
          'external': (await getExternalStorageDirectory())!,
      };
      final Map<String, String> failures = <String, String>{};

      for (final MapEntry<String, Directory>(:String key, :Directory value)
          in folders.entries) {
        final Directory folder = await Directory(
          '${value.path}${Platform.pathSeparator}darudb_engine_test',
        ).create(recursive: true);

        try {
          final Database db = await Database.openAsync(
            '${folder.path}${Platform.pathSeparator}folder.darudb',
            schema: sampleSchema,
          );

          try {
            await db.writeAsync(
              (AsyncWriteTransaction txn) =>
                  txn.collection(organizationSchema).insert(_organization(key)),
            );
            expect(await _count(db), 1, reason: key);
          } finally {
            await db.closeAsync();
          }
        } on DaruException catch (error) {
          failures[key] = '${value.path}: ${error.code}, ${error.message}';
        } finally {
          await folder.delete(recursive: true);
        }
      }

      expect(failures, isEmpty);
    },
    // A desktop app's folders are the user's own, which a test leaves alone.
    skip: !_onDevice,
  );
}
