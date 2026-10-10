// The sample's database: the one file it keeps, and every operation the
// screens ask of it.
//
// The file is plain or encrypted with `samplePassword`, as the screens chose
// when they last made it, and opening finds out which: a plain open of an
// encrypted file fails with `KEY_REQUIRED`, and the file is opened again with
// the password.
//
// Every call goes through the `Future` API, which does the engine's work on
// threads of the native library: a call through `dart:ffi` holds the isolate,
// and this is the isolate that draws the screens. Objects reach the screens
// as `SampleRow`s and come back the same way, so the screens handle the
// three collections alike; a change is written back whole with `put`, the
// way an immutable object is changed.
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/model.dart';
import 'package:darudb_sample/src/sample.dart';

const String sampleFileName = 'sample.darudb';

/// The password of an encrypted sample file. The sample keeps it in its code
/// so that either kind of file opens without asking for it, which is fine for
/// sample data and nothing else: an app keeps its key in the operating
/// system's keystore, as the encryption guide shows.
const String samplePassword = 'darudb sample';

/// How many objects a write transaction of a sample run inserts.
const int _batchSize = 5000;

/// The most problems a check reports to the screens.
const int _problemsShown = 50;

final class SampleInfo {
  const SampleInfo({
    required this.path,
    required this.bytes,
    required this.pageSize,
    required this.formatVersion,
    required this.schemaVersion,
    required this.encrypted,
    required this.engineVersion,
    required this.counts,
  });

  final String path;

  /// The size of the file, in bytes.
  final int bytes;
  final int pageSize;
  final int formatVersion;
  final int? schemaVersion;
  final bool encrypted;
  final String engineVersion;

  /// How many objects each collection holds.
  final Map<SampleCollection, int> counts;
}

final class SampleSort {
  const SampleSort(this.field, {this.descending = false});

  final String field;
  final bool descending;
}

final class ListResult {
  const ListResult(this.rows, this.total);

  final List<SampleRow> rows;

  /// How many objects the filter finds, past the page shown.
  final int total;
}

enum SeedStage { pools, organizations, people, posts }

final class SeedProgress {
  const SeedProgress(this.stage, this.done, this.total);

  final SeedStage stage;
  final int done;
  final int total;
}

final class SeedReport {
  const SeedReport({
    required this.plan,
    required this.generateMs,
    required this.insertMs,
  });

  final Plan plan;

  /// Time spent making the objects, in milliseconds.
  final int generateMs;

  /// Time spent in write transactions, in milliseconds.
  final int insertMs;
}

final class CheckSummary {
  const CheckSummary({
    required this.ok,
    required this.pagesChecked,
    required this.objectsChecked,
    required this.problems,
  });

  final bool ok;
  final int pagesChecked;
  final int objectsChecked;
  final List<String> problems;
}

/// The run's pools, filled on another isolate while this one keeps drawing
/// the screens. A function of its own, so that the closure sent to the
/// isolate holds the seed and nothing of the store's.
Future<SampleData> _sampleData(int seed) => Isolate.run(() => SampleData(seed));

SampleRow _organizationRow(Organization organization) => <String, Object?>{
  'code': organization.code,
  'name': organization.name,
  'kind': organization.kind,
  'industry': organization.industry,
  'language': organization.language,
  'founded': organization.founded,
};

Organization _organizationOf(SampleRow row) => Organization(
  code: row['code'] as String,
  name: row['name'] as String,
  kind: row['kind'] as String,
  industry: row['industry'] as String?,
  language: row['language'] as String,
  founded: row['founded'] as int,
);

SampleRow _personRow(Person person) => <String, Object?>{
  'id': person.id,
  'color': person.color,
  'name': person.name,
  'nickname': person.nickname,
  'email': person.email,
  'age': person.age,
  'gender': person.gender,
  'language': person.language,
  'location': person.location,
  'organization': person.organization?.key,
  'tags': person.tags,
  'active': person.active,
  'score': person.score,
  'joinedAt': person.joinedAt,
};

Person _personOf(SampleRow row, {int? id}) {
  final String? organization = row['organization'] as String?;

  return Person(
    id: id,
    color: row['color'] as Uint8List,
    name: row['name'] as String,
    nickname: row['nickname'] as String,
    email: row['email'] as String?,
    age: row['age'] as int,
    gender: row['gender'] as String,
    language: row['language'] as String,
    location: row['location'] as Place?,
    organization: organization == null
        ? null
        : Link<Organization>(organization),
    tags: row['tags'] as List<String>,
    active: row['active'] as bool,
    score: row['score'] as double,
    joinedAt: row['joinedAt'] as int,
  );
}

SampleRow _postRow(Post post) => <String, Object?>{
  'id': post.id,
  'author': post.author.key,
  'title': post.title,
  'body': post.body,
  'language': post.language,
  'tags': post.tags,
  'likes': post.likes,
  'pinned': post.pinned,
  'createdAt': post.createdAt,
};

Post _postOf(SampleRow row, {int? id}) => Post(
  id: id,
  author: Link<Person>(row['author'] as int),
  title: row['title'] as String,
  body: row['body'] as String,
  language: row['language'] as String,
  tags: row['tags'] as List<String>?,
  likes: row['likes'] as int,
  pinned: row['pinned'] as bool,
  createdAt: row['createdAt'] as int,
);

/// The text of a list request in the query language: its filter, sort and
/// page. The sort's field is one of the names the list offers.
String _queryText({
  required String filter,
  required SampleSort? sort,
  required int offset,
  required int limit,
}) {
  final List<String> parts = <String>[
    filter.trim(),
    if (sort != null)
      'SORT BY ${sort.field} ${sort.descending ? 'DESC' : 'ASC'}',
    'LIMIT $limit OFFSET $offset',
  ];

  return parts.where((String part) => part.isNotEmpty).join(' ');
}

final class SampleStore {
  SampleStore._(this.path, this._db);

  final String path;
  Database _db;

  /// Opens the sample's file in [directory], plain or encrypted, creating a
  /// plain one, and the folder, when they are not there.
  static Future<SampleStore> open(String directory) async {
    await Directory(directory).create(recursive: true);

    final String path = '$directory${Platform.pathSeparator}$sampleFileName';
    Database db;

    try {
      db = await Database.openAsync(path, schema: sampleSchema);
    } on DaruException catch (error) {
      if (error.code != 'KEY_REQUIRED') {
        rethrow;
      }

      db = await Database.openAsync(
        path,
        schema: sampleSchema,
        password: samplePassword,
      );
    }

    return SampleStore._(path, db);
  }

  Future<SampleInfo> info() async {
    final List<int> counts = await _db.readAsync(
      (AsyncReadTransaction txn) => Future.wait(<Future<int>>[
        txn.collection(organizationSchema).count(),
        txn.collection(personSchema).count(),
        txn.collection(postSchema).count(),
      ]),
    );

    return SampleInfo(
      path: path,
      bytes: await File(path).length(),
      pageSize: _db.pageSize,
      formatVersion: _db.formatVersion,
      schemaVersion: _db.schemaVersion,
      encrypted: _db.isEncrypted,
      engineVersion: engineVersion,
      counts: <SampleCollection, int>{
        SampleCollection.organizations: counts[0],
        SampleCollection.people: counts[1],
        SampleCollection.posts: counts[2],
      },
    );
  }

  Future<ListResult> list({
    required SampleCollection collection,
    required String filter,
    required SampleSort? sort,
    required int offset,
    required int limit,
  }) {
    final String text = _queryText(
      filter: filter,
      sort: sort,
      offset: offset,
      limit: limit,
    );

    return switch (collection) {
      SampleCollection.organizations => _listOf(
        organizationSchema,
        _organizationRow,
        text,
        filter,
      ),
      SampleCollection.people => _listOf(
        personSchema,
        _personRow,
        text,
        filter,
      ),
      SampleCollection.posts => _listOf(postSchema, _postRow, text, filter),
    };
  }

  /// Inserts the object [row] describes and returns its key.
  Future<Object> insert(SampleCollection collection, SampleRow row) =>
      _db.writeAsync((AsyncWriteTransaction txn) async {
        return switch (collection) {
          SampleCollection.organizations =>
            await txn
                .collection(organizationSchema)
                .insert(_organizationOf(row)),
          SampleCollection.people =>
            await txn.collection(personSchema).insert(_personOf(row)),
          SampleCollection.posts =>
            await txn.collection(postSchema).insert(_postOf(row)),
        };
      });

  /// Writes the object [row] describes over the one under [key].
  Future<void> replace(
    SampleCollection collection,
    Object key,
    SampleRow row,
  ) => _db.writeAsync((AsyncWriteTransaction txn) async {
    switch (collection) {
      case SampleCollection.organizations:
        await txn
            .collection(organizationSchema)
            .put(_organizationOf(<String, Object?>{...row, 'code': key}));
      case SampleCollection.people:
        await txn.collection(personSchema).put(_personOf(row, id: key as int));
      case SampleCollection.posts:
        await txn.collection(postSchema).put(_postOf(row, id: key as int));
    }
  });

  Future<bool> remove(SampleCollection collection, Object key) =>
      _db.writeAsync((AsyncWriteTransaction txn) {
        return switch (collection) {
          SampleCollection.organizations =>
            txn.collection(organizationSchema).delete(key as String),
          SampleCollection.people =>
            txn.collection(personSchema).delete(key as int),
          SampleCollection.posts =>
            txn.collection(postSchema).delete(key as int),
        };
      });

  /// Inserts a run of sample data: [people] people, an organization for
  /// every fifty of them and three posts each, in write transactions of
  /// [_batchSize] objects. The report keeps the time spent making objects
  /// apart from the time spent writing them, which is the engine's.
  Future<SeedReport> seed({
    required int people,
    required int seed,
    required void Function(SeedProgress progress) onProgress,
  }) async {
    final Plan plan = Plan(people);
    final Stopwatch generating = Stopwatch();
    final Stopwatch inserting = Stopwatch();

    onProgress(const SeedProgress(SeedStage.pools, 0, 1));
    generating.start();

    final SampleData data = await _sampleData(seed);

    generating.stop();

    final ({int organization, int person, int post}) starts =
        await _nextNumbers();
    final List<String> codes = <String>[];
    final List<Author> authors = <Author>[];

    Future<void> runBatches<T>({
      required SeedStage stage,
      required int total,
      required T Function(int index) make,
      required Future<void> Function(List<T> batch) write,
    }) async {
      for (int done = 0; done < total; done += _batchSize) {
        final int count = total - done < _batchSize ? total - done : _batchSize;

        generating.start();

        final List<T> batch = <T>[
          for (int i = 0; i < count; i += 1) make(done + i),
        ];

        generating.stop();
        inserting.start();
        await write(batch);
        inserting.stop();
        onProgress(SeedProgress(stage, done + count, total));
      }
    }

    await runBatches<Organization>(
      stage: SeedStage.organizations,
      total: plan.organizations,
      make: (int i) => data.organization(starts.organization + i),
      write: (List<Organization> batch) async {
        await _db.writeAsync(
          (AsyncWriteTransaction txn) =>
              txn.collection(organizationSchema).insertMany(batch),
        );
        codes.addAll(batch.map((Organization made) => made.code));
      },
    );
    await runBatches<Person>(
      stage: SeedStage.people,
      total: plan.people,
      make: (int i) => data.person(starts.person + i, codes),
      write: (List<Person> batch) async {
        final List<int> keys = await _db.writeAsync(
          (AsyncWriteTransaction txn) =>
              txn.collection(personSchema).insertMany(batch),
        );

        for (int i = 0; i < keys.length; i += 1) {
          authors.add(Author(keys[i], batch[i].language));
        }
      },
    );
    if (authors.isEmpty) {
      return SeedReport(
        plan: plan,
        generateMs: generating.elapsedMilliseconds,
        insertMs: inserting.elapsedMilliseconds,
      );
    }

    await runBatches<Post>(
      stage: SeedStage.posts,
      total: plan.posts,
      make: (int i) => data.post(starts.post + i, authors),
      write: (List<Post> batch) async {
        await _db.writeAsync(
          (AsyncWriteTransaction txn) =>
              txn.collection(postSchema).insertMany(batch),
        );
      },
    );

    return SeedReport(
      plan: plan,
      generateMs: generating.elapsedMilliseconds,
      insertMs: inserting.elapsedMilliseconds,
    );
  }

  Future<CheckSummary> check() async {
    final CheckReport report = await _db.checkAsync();

    return CheckSummary(
      ok: report.ok,
      pagesChecked: report.pagesChecked,
      objectsChecked: report.objectsChecked,
      problems: <String>[
        for (final CheckProblem problem in report.problems.take(_problemsShown))
          problem.message,
      ],
    );
  }

  Future<CompactReport> compact() => _db.compactAsync();

  /// Closes the file, deletes it, and starts again from an empty one,
  /// encrypted with [samplePassword] or not.
  Future<SampleInfo> reset({required bool encrypted}) async {
    await _db.closeAsync();
    await File(path).delete();
    _db = await Database.openAsync(
      path,
      schema: sampleSchema,
      password: encrypted ? samplePassword : null,
    );

    return info();
  }

  Future<void> close() => _db.closeAsync();

  Future<ListResult> _listOf<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
    SampleRow Function(T object) rowOf,
    String text,
    String filter,
  ) async {
    final (List<T> objects, int total) = await _db.readAsync((
      AsyncReadTransaction txn,
    ) async {
      final AsyncReadCollection<T, Q, K> collection = txn.collection(schema);
      final Future<List<T>> objects = collection.findText(text);
      final Future<int> total = collection.countText(filter);

      // Both calls are under way before either is awaited, so when a filter
      // that does not parse fails the first, the second's failure is let go
      // rather than left unhandled.
      try {
        return (await objects, await total);
      } finally {
        total.ignore();
      }
    });

    return ListResult(<SampleRow>[
      for (final T object in objects) rowOf(object),
    ], total);
  }

  /// The number each kind of sample object continues from: one past the
  /// highest organization code, person and post in the file, so that a run
  /// never repeats a code, a nickname or an email already there.
  Future<({int organization, int person, int post})> _nextNumbers() async {
    final (Organization? organization, Person? person, Post? post) = await _db
        .readAsync((AsyncReadTransaction txn) async {
          final Future<Organization?> organization = txn
              .collection(organizationSchema)
              .findOneText(r'code STARTSWITH $0 SORT BY code DESC', <Object?>[
                organizationPrefix,
              ]);
          final Future<Person?> person = txn
              .collection(personSchema)
              .findOneText('SORT BY id DESC');
          final Future<Post?> post = txn
              .collection(postSchema)
              .findOneText('SORT BY id DESC');

          return (await organization, await person, await post);
        });
    final int lastCode = organization == null
        ? 0
        : int.tryParse(
                organization.code.substring(organizationPrefix.length),
              ) ??
              0;

    return (
      organization: lastCode + 1,
      person: (person?.id ?? 0) + 1,
      post: (post?.id ?? 0) + 1,
    );
  }
}
