import 'dart:io';

import 'package:darudb/darudb.dart';
import 'package:test/test.dart';

import 'models.dart';

const _articles = [
  Article(
    title: 'Quick Start',
    body: 'Open a database and write.',
    tags: ['guide', 'getting started'],
    motto: 'Quick and quiet',
    margin: Margin(note: 'checked twice'),
  ),
  Article(
    title: 'Queries',
    body: 'Write a query quickly.',
    tags: ['guide'],
    rank: 2,
  ),
  Article(title: '빠른 검색', body: '빠른 검색을 한다', motto: 'quick'),
  Article(title: '東京', body: '東京都に住む', margin: Margin()),
  Article(title: '?!'),
];

Matcher failsWith(String code) =>
    throwsA(isA<DaruException>().having((e) => e.code, 'code', code));

final class _NoQuery extends QueryBuilder<Object?> {}

void main() {
  late Directory directory;
  late String path;

  setUp(() {
    directory = Directory.systemTemp.createTempSync('darudb-dart-');
    path = '${directory.path}/app.darudb';
  });

  tearDown(() => directory.deleteSync(recursive: true));

  Database opened() {
    final db = Database.open(path, schema: const Schema(1, [articleSchema]));

    addTearDown(db.close);
    db.write((txn) => txn.collection(articleSchema).insertMany(_articles));

    return db;
  }

  test('a schema declares text indexes on fields and on a path', () {
    expect(
      [
        for (final field in articleSchema.fields)
          if (field.index || field.textIndex)
            '${field.name} ${field.index} ${field.textIndex}',
      ],
      ['title true true', 'text false true', 'tags false true'],
    );
    expect(
      [
        for (final index in articleSchema.indexes)
          '${index.fields.join(',')} ${index.unique} ${index.text}',
      ],
      ['margin.note false true'],
    );
  });

  test('matches finds the objects whose text has every word', () {
    final db = opened();

    db.read((txn) {
      final articles = txn.collection(articleSchema);
      List<int?> found(Condition Function(ArticleQuery q) condition) => [
        for (final article in articles.find((q) => q.where(condition(q))))
          article.id,
      ];

      // Every word, in any order and any case; a star makes a word a start.
      expect(found((q) => q.title.matches('quick')), [1]);
      expect(found((q) => q.body.matches('WRITE')), [1, 2]);
      expect(found((q) => q.body.matches('write quick*')), [2]);
      expect(found((q) => q.body.matches('quick')), isEmpty);
      expect(found((q) => q.body.matches('write cat')), isEmpty);
      // A search without a word finds nothing.
      expect(found((q) => q.body.matches('')), isEmpty);
      expect(found((q) => q.body.matches('* ?!')), isEmpty);
      // Hangul by the words between spaces, and Han and kana in pairs.
      expect(found((q) => q.body.matches('검색')), isEmpty);
      expect(found((q) => q.body.matches('검색*')), [3]);
      expect(found((q) => q.body.matches('東京')), [4]);
      expect(found((q) => q.body.matches('京都に')), [4]);
      expect(found((q) => q.body.matches('住')), [4]);
      expect(found((q) => q.body.matches('大阪')), isEmpty);
      // A list: one string has every word.
      expect(found((q) => q.tags.matches('guide')), [1, 2]);
      expect(found((q) => q.tags.matches('getting started')), [1]);
      expect(found((q) => q.tags.matches('guide started')), isEmpty);
      // Inside an embedded object, and a field without a text index.
      expect(found((q) => q.margin.note.matches('twice')), [1]);
      expect(found((q) => q.motto.matches('qui*')), [1, 3]);
      expect(found((q) => q.motto.matches('quick and')), [1]);
      // With other conditions, and left alone by ignoreCase.
      expect(found((q) => q.body.matches('write') & q.rank.equals(2)), [2]);
      expect(found((q) => q.body.matches('東京') | q.tags.matches('started')), [
        1,
        4,
      ]);
      expect(found((q) => ~q.body.matches('write')), [3, 4, 5]);
      expect(found((q) => q.body.matches('WRITE').ignoreCase()), [1, 2]);
      expect(articles.count((q) => q.where(q.body.matches('write'))), 2);

      // The query language, with the search in the text or passed with it.
      expect(articles.countText('text MATCHES "write quick*"'), 1);
      expect(articles.findText(r'text MATCHES $0', ['검색*']).map((a) => a.id), [
        3,
      ]);
      expect(
        articles
            .findPrepared(db.prepare(articleSchema, r'tags MATCHES $0'), [
              'getting started',
            ])
            .map((a) => a.id),
        [1],
      );
      expect(
        () => articles.countText('rank MATCHES "1"'),
        failsWith('INVALID_QUERY'),
      );
    });

    expect(db.check().ok, isTrue);
  });

  test('a text index follows the changes of its objects', () {
    final db = opened();

    db.write((txn) {
      txn.collection(articleSchema)
        ..update(1, (q) => [q.body.set('Close the file.')])
        ..delete(2)
        ..put(const Article(id: 5, title: 'Writers', body: 'write again'));
    });
    db.read((txn) {
      final articles = txn.collection(articleSchema);

      expect(
        articles.find((q) => q.where(q.body.matches('write'))).map((a) => a.id),
        [5],
      );
      expect(articles.count((q) => q.where(q.body.matches('close file'))), 1);
      expect(articles.count((q) => q.where(q.title.matches('writ*'))), 1);
    });
    expect(db.check().ok, isTrue);
  });

  test('the Future API searches as the synchronous one does', () async {
    opened().close();

    final db = await Database.openAsync(
      path,
      schema: const Schema(1, [articleSchema]),
    );

    await db.readAsync((txn) async {
      final articles = txn.collection(articleSchema);

      expect(
        (await articles.find(
          (q) => q.where(q.body.matches('write')),
        )).map((a) => a.id),
        [1, 2],
      );
      expect(
        await articles.count((q) => q.where(q.tags.matches('guide star*'))),
        0,
      );
      expect(await articles.countText(r'text MATCHES $0', ['東京']), 1);
    });
    await db.closeAsync();
  });

  test('the stored schema says which indexes are text indexes', () {
    opened().close();

    final db = Database.open(path);

    addTearDown(db.close);

    final articles = db.storedCollection('articles');

    expect(
      [
        for (final field in articles.fields)
          if (field.index || field.textIndex)
            '${field.name} ${field.index} ${field.textIndex}',
      ],
      ['title true true', 'text false true', 'tags false true'],
    );
    expect(
      [
        for (final index in articles.indexes)
          '${index.fields.join(',')} ${index.unique} ${index.text}',
      ],
      ['margin.note false true'],
    );
    // As maps, by the names the file stores.
    db.read((txn) {
      final all = txn.collection(articles);

      expect(
        all
            .find((q) => q.where(q.stringField('text').matches('write')))
            .map((article) => article['id']),
        [1, 2],
      );
      expect(
        all.count((q) => q.where(q.stringListField('tags').matches('guide'))),
        2,
      );
      expect(
        all.count(
          (q) => q.where(q.stringField('margin.note').matches('checked')),
        ),
        1,
      );
    });
  });

  test('a text index is on one field of text, and is not unique', () {
    Database declare({
      List<IndexSpec> indexes = const [],
      bool countText = false,
    }) => Database.open(
      path,
      schema: Schema(1, [
        CollectionSchema<Object?, _NoQuery, int>(
          name: 'notes',
          autoKey: true,
          fields: [
            const FieldSpec('id', IntKind()),
            const FieldSpec('body', StringKind(), optional: true),
            FieldSpec('count', const IntKind(), textIndex: countText),
          ],
          indexes: indexes,
          writeField: (_, _, _) {},
          read: (_) => null,
          query: _NoQuery.new,
        ),
      ]),
    );

    expect(() => declare(countText: true), failsWith('INVALID_ARGUMENT'));
    expect(
      () => declare(
        indexes: const [
          IndexSpec(['body'], text: true, unique: true),
        ],
      ),
      failsWith('INVALID_ARGUMENT'),
    );
    expect(
      () => declare(
        indexes: const [
          IndexSpec(['body', 'count'], text: true),
        ],
      ),
      failsWith('INVALID_ARGUMENT'),
    );

    declare(
      indexes: const [
        IndexSpec(['body'], text: true),
      ],
    ).close();

    final db = Database.open(path);

    addTearDown(db.close);
    expect(db.storedCollection('notes').fields[1].textIndex, isTrue);
  });

  test('a file kept from upgrading refuses a text index', () {
    expect(
      () => Database.open(
        path,
        schema: const Schema(1, [articleSchema]),
        upgradeFormat: false,
      ),
      failsWith('INVALID_ARGUMENT'),
    );
  });
}
