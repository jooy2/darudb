import 'dart:io';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/store.dart';

void main() {
  late Directory directory;
  late SampleStore store;

  setUp(() async {
    directory = Directory.systemTemp.createTempSync('darudb_sample_store');
    store = await SampleStore.open(directory.path);
  });

  tearDown(() async {
    await store.close();
    directory.deleteSync(recursive: true);
  });

  test(
    'a run inserts its plan, and a second run continues its numbers',
    () async {
      final List<SeedStage> stages = <SeedStage>[];

      await store.seed(
        people: 1000,
        seed: 7,
        onProgress: (SeedProgress progress) => stages.add(progress.stage),
      );
      await store.seed(people: 1000, seed: 7, onProgress: (_) {});

      final SampleInfo info = await store.info();

      expect(info.counts, <SampleCollection, int>{
        SampleCollection.organizations: 40,
        SampleCollection.people: 2000,
        SampleCollection.posts: 6000,
      });
      expect(stages.toSet(), SeedStage.values.toSet());
      expect((await store.check()).ok, isTrue);
    },
  );

  test('the list filters, sorts and pages in the engine', () async {
    await store.seed(people: 1000, seed: 3, onProgress: (_) {});

    final ListResult page = await store.list(
      collection: SampleCollection.people,
      filter: 'age >= 65',
      sort: const SampleSort('age', descending: true),
      offset: 0,
      limit: 20,
    );
    final List<int> ages = <int>[
      for (final SampleRow row in page.rows) row['age'] as int,
    ];

    expect(page.total, greaterThan(0));
    expect(ages, hasLength(20));
    expect(ages.every((int age) => age >= 65), isTrue);
    expect(ages, orderedEquals(<int>[...ages]..sort((int a, int b) => b - a)));
  });

  test(
    'a row goes in, changes, is refused when it repeats a nickname, and goes',
    () async {
      final SampleRow row = <String, Object?>{
        'color': Uint8List.fromList(<int>[1, 2, 3]),
        'name': 'End To End',
        'nickname': 'e2e-tester',
        'email': null,
        'age': 33,
        'gender': 'female',
        'language': 'en',
        'location': null,
        'organization': null,
        'tags': <String>['sample'],
        'active': true,
        'score': 0.0,
        'joinedAt': 0,
      };
      final Object key = await store.insert(SampleCollection.people, row);

      await store.replace(SampleCollection.people, key, <String, Object?>{
        ...row,
        'age': 34,
      });

      final ListResult found = await store.list(
        collection: SampleCollection.people,
        filter: 'nickname == "e2e-tester"',
        sort: null,
        offset: 0,
        limit: 10,
      );

      expect(found.rows.single['age'], 34);
      await expectLater(
        store.insert(SampleCollection.people, row),
        throwsA(
          isA<DaruException>().having(
            (DaruException e) => e.code,
            'code',
            'DUPLICATE_KEY',
          ),
        ),
      );
      expect(await store.remove(SampleCollection.people, key), isTrue);
      expect((await store.info()).counts[SampleCollection.people], 0);
    },
  );

  test('a filter that does not parse fails with INVALID_QUERY', () async {
    await expectLater(
      store.list(
        collection: SampleCollection.people,
        filter: 'age >>',
        sort: null,
        offset: 0,
        limit: 10,
      ),
      throwsA(
        isA<DaruException>().having(
          (DaruException e) => e.code,
          'code',
          'INVALID_QUERY',
        ),
      ),
    );
  });

  test('reset leaves an empty file', () async {
    await store.seed(people: 1000, seed: 1, onProgress: (_) {});

    final SampleInfo info = await store.reset(encrypted: false);

    expect(info.counts.values.every((int count) => count == 0), isTrue);
    expect(info.encrypted, isFalse);
  });

  test(
    'an encrypted file opens again with the password, found out by its code',
    () async {
      expect((await store.reset(encrypted: true)).encrypted, isTrue);
      await store.seed(people: 1000, seed: 1, onProgress: (_) {});
      await store.close();

      store = await SampleStore.open(directory.path);

      final SampleInfo info = await store.info();

      expect(info.encrypted, isTrue);
      expect(info.counts[SampleCollection.people], 1000);
      expect((await store.reset(encrypted: false)).encrypted, isFalse);
    },
  );
}
