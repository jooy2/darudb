/// The complex suite on SQLite through the `sqlite3` package, with the
/// settings of the simple suite: a write-ahead log with `synchronous = FULL`,
/// `fullfsync`, and a 32 MiB page cache. The schema is what an SQL
/// application would write: a foreign key for each link, the address's
/// fields as columns of their own, and a table for each list, with an index
/// on every column the other stores index. A row is read with its tags,
/// through a subquery, and made into an object, so that every store makes
/// the same objects.
library;

import 'package:sqlite3/sqlite3.dart';

import 'common.dart' show Digest, Rows;
import 'complex.dart';

const _person =
    'SELECT id, name, email, age, country, city, zip, score, active, '
    'joined_at, organization, '
    '(SELECT group_concat(tag) FROM person_tags WHERE person = people.id) '
    'FROM people';

const _post =
    'SELECT id, author, title, body, likes, language, created_at, pinned, '
    '(SELECT group_concat(tag) FROM post_tags WHERE post = posts.id) '
    'FROM posts';

const _schema = '''PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA fullfsync = ON;
PRAGMA cache_size = -32768;
CREATE TABLE organizations (
  code TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
  country TEXT NOT NULL, founded INTEGER NOT NULL);
CREATE INDEX organizations_kind ON organizations (kind);
CREATE INDEX organizations_country ON organizations (country);
CREATE TABLE people (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
  age INTEGER NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL,
  zip TEXT NOT NULL, score REAL NOT NULL, active INTEGER NOT NULL,
  joined_at INTEGER NOT NULL,
  organization TEXT NOT NULL REFERENCES organizations (code));
CREATE INDEX people_age ON people (age);
CREATE INDEX people_country ON people (country);
CREATE INDEX people_joined_at ON people (joined_at);
CREATE INDEX people_organization ON people (organization);
CREATE TABLE person_tags (person INTEGER NOT NULL, tag TEXT NOT NULL);
CREATE INDEX person_tags_person ON person_tags (person);
CREATE INDEX person_tags_tag ON person_tags (tag, person);
CREATE TABLE posts (
  id INTEGER PRIMARY KEY, author INTEGER NOT NULL REFERENCES people (id),
  title TEXT NOT NULL, body TEXT NOT NULL, likes INTEGER NOT NULL,
  language TEXT NOT NULL, created_at INTEGER NOT NULL, pinned INTEGER NOT NULL);
CREATE INDEX posts_author ON posts (author);
CREATE INDEX posts_likes ON posts (likes);
CREATE INDEX posts_language ON posts (language);
CREATE INDEX posts_created_at ON posts (created_at);
CREATE TABLE post_tags (post INTEGER NOT NULL, tag TEXT NOT NULL);
CREATE INDEX post_tags_post ON post_tags (post);
CREATE INDEX post_tags_tag ON post_tags (tag, post);''';

List<String> _tags(Object? text) =>
    text == null ? const [] : (text as String).split(',');

PersonFields _personOf(Row row) => PersonFields(
  id: row.columnAt(0) as int,
  name: row.columnAt(1) as String,
  email: row.columnAt(2) as String,
  age: row.columnAt(3) as int,
  country: row.columnAt(4) as String,
  city: row.columnAt(5) as String,
  zip: row.columnAt(6) as String,
  score: row.columnAt(7) as double,
  active: row.columnAt(8) == 1,
  joinedAt: row.columnAt(9) as int,
  organization: row.columnAt(10) as String,
  tags: _tags(row.columnAt(11)),
);

PostFields _postOf(Row row) => PostFields(
  id: row.columnAt(0) as int,
  author: row.columnAt(1) as int,
  title: row.columnAt(2) as String,
  body: row.columnAt(3) as String,
  likes: row.columnAt(4) as int,
  language: row.columnAt(5) as String,
  createdAt: row.columnAt(6) as int,
  pinned: row.columnAt(7) == 1,
  tags: _tags(row.columnAt(8)),
);

void _people(PreparedStatement statement, List<Object?> parameters, Digest d) {
  for (final row in statement.select(parameters)) {
    final p = _personOf(row);

    d.add(p.id, p.age);
  }
}

void _posts(PreparedStatement statement, List<Object?> parameters, Digest d) {
  for (final row in statement.select(parameters)) {
    final p = _postOf(row);

    d.add(p.id, p.likes);
  }
}

void _count(PreparedStatement statement, List<Object?> parameters, Digest d) {
  d.number(statement.select(parameters).first.columnAt(0) as int);
}

void run(String directory, Rows rows) {
  final db = sqlite3.open('$directory/complex.sqlite');

  db.execute(_schema);

  // The objects are made before the load's time starts.
  final organizations = [
    for (var n = 0; n < organizationCount; n++) organization(n),
  ];
  final people = [for (var n = 0; n < peopleCount; n++) person(n)];
  final posts = [for (var m = 0; m < postCount; m++) post(m)];
  final insertOrganization = db.prepare(
    'INSERT INTO organizations (code, name, kind, country, founded) '
    'VALUES (?, ?, ?, ?, ?)',
  );
  final insertPerson = db.prepare(
    'INSERT INTO people (name, email, age, country, city, zip, score, active, '
    'joined_at, organization) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
  );
  final insertPersonTag = db.prepare(
    'INSERT INTO person_tags (person, tag) VALUES (?, ?)',
  );
  final insertPost = db.prepare(
    'INSERT INTO posts (author, title, body, likes, language, created_at, '
    'pinned) VALUES (?, ?, ?, ?, ?, ?, ?)',
  );
  final insertPostTag = db.prepare(
    'INSERT INTO post_tags (post, tag) VALUES (?, ?)',
  );

  rows.all(ComplexRows.load.$1, ComplexRows.load.$2, (_) {
    for (final part in batches(organizations)) {
      db.execute('BEGIN');

      for (final o in part) {
        insertOrganization.execute([
          o.code,
          o.name,
          o.kind,
          o.country,
          o.founded,
        ]);
      }

      db.execute('COMMIT');
    }

    for (final part in batches(people)) {
      db.execute('BEGIN');

      for (final p in part) {
        insertPerson.execute([
          p.name,
          p.email,
          p.age,
          p.country,
          p.city,
          p.zip,
          p.score,
          p.active ? 1 : 0,
          p.joinedAt,
          p.organization,
        ]);

        final id = db.lastInsertRowId;

        for (final tag in p.tags) {
          insertPersonTag.execute([id, tag]);
        }
      }

      db.execute('COMMIT');
    }

    for (final part in batches(posts)) {
      db.execute('BEGIN');

      for (final p in part) {
        insertPost.execute([
          p.author,
          p.title,
          p.body,
          p.likes,
          p.language,
          p.createdAt,
          p.pinned ? 1 : 0,
        ]);

        final id = db.lastInsertRowId;

        for (final tag in p.tags) {
          insertPostTag.execute([id, tag]);
        }
      }

      db.execute('COMMIT');
    }
  });

  final byId = db.prepare('$_person WHERE id = ?');
  final organizationOf = db.prepare(
    'SELECT code, name, kind, country, founded FROM organizations '
    'WHERE code = ?',
  );
  final newest = db.prepare(
    '$_post WHERE author = ? ORDER BY created_at DESC, id LIMIT 5',
  );
  final filter = db.prepare(
    '$_person WHERE age BETWEEN ? AND ? AND country = ? AND active = 1 '
    'ORDER BY score DESC, id LIMIT 50',
  );
  final either = db.prepare(
    'SELECT count(*) FROM people WHERE country IN (?, ?) OR age >= 78',
  );
  final prefix = db.prepare(
    '$_person WHERE substr(name, 1, length(?1)) = ?1 ORDER BY id',
  );
  final page = db.prepare('$_person ORDER BY joined_at, id LIMIT 50 OFFSET ?');
  final authorCountry = db.prepare(
    '$_post WHERE likes >= 900 AND author IN '
    '(SELECT id FROM people WHERE country = ?) '
    'ORDER BY created_at DESC, id LIMIT 20',
  );
  final twoLinks = db.prepare(
    'SELECT count(*) FROM posts JOIN people ON people.id = posts.author '
    'JOIN organizations ON organizations.code = people.organization '
    'WHERE organizations.kind = ? AND posts.language = ?',
  );
  final tag = db.prepare(
    '$_post WHERE id IN (SELECT post FROM post_tags WHERE tag = ?) '
    'ORDER BY likes DESC, id LIMIT 20',
  );
  final text = db.prepare(
    'SELECT count(*) FROM posts WHERE instr(body, ?) > 0',
  );
  final likes = db.prepare(
    'SELECT count(*) FROM posts WHERE likes >= 500 AND language = ?',
  );

  db.execute('BEGIN');
  rows.each(ComplexRows.detail.$1, ComplexRows.detail.$2, (round, d) {
    final found = byId.select([Runs.detail(round)]);

    if (found.isEmpty) {
      return;
    }

    final p = _personOf(found.first);

    d.add(p.id, p.age);

    final o = organizationOf.select([p.organization]);

    if (o.isNotEmpty) {
      final row = o.first;
      final organization = OrganizationFields(
        row.columnAt(0) as String,
        row.columnAt(1) as String,
        row.columnAt(2) as String,
        row.columnAt(3) as String,
        row.columnAt(4) as int,
      );

      d.add(organization.founded, 0);
    }

    _posts(newest, [p.id], d);
  });
  rows.each(ComplexRows.filter.$1, ComplexRows.filter.$2, (round, d) {
    final (age, country) = Runs.filter(round);

    _people(filter, [age, age + 9, country], d);
  });
  rows.each(ComplexRows.either.$1, ComplexRows.either.$2, (round, d) {
    final (first, second) = Runs.either(round);

    _count(either, [first, second], d);
  });
  rows.each(
    ComplexRows.prefix.$1,
    ComplexRows.prefix.$2,
    (round, d) => _people(prefix, [Runs.prefix(round)], d),
  );
  rows.each(
    ComplexRows.page.$1,
    ComplexRows.page.$2,
    (round, d) => _people(page, [Runs.offset(round)], d),
  );
  rows.each(
    ComplexRows.authorCountry.$1,
    ComplexRows.authorCountry.$2,
    (round, d) => _posts(authorCountry, [Runs.authorCountry(round)], d),
  );
  rows.each(ComplexRows.twoLinks.$1, ComplexRows.twoLinks.$2, (round, d) {
    final (kind, language) = Runs.twoLinks(round);

    _count(twoLinks, [kind, language], d);
  });
  rows.each(
    ComplexRows.tag.$1,
    ComplexRows.tag.$2,
    (round, d) => _posts(tag, [Runs.tag(round)], d),
  );
  rows.each(
    ComplexRows.text.$1,
    ComplexRows.text.$2,
    (round, d) => _count(text, [Runs.phrase(round)], d),
  );
  rows.each(
    ComplexRows.count.$1,
    ComplexRows.count.$2,
    (round, d) => _count(likes, [Runs.language(round)], d),
  );
  db.execute('COMMIT');

  final update = db.prepare(
    'UPDATE people SET age = (age + 1) % 80 WHERE country = ?',
  );

  rows.all(ComplexRows.update, updatedCount(), (d) {
    db.execute('BEGIN');

    for (final country in Runs.updated) {
      update.execute([country]);
      d.number(db.updatedRows);
    }

    db.execute('COMMIT');
  });

  final deletePostTags = db.prepare(
    'DELETE FROM post_tags WHERE post IN (SELECT id FROM posts WHERE author = ?)',
  );
  final deletePosts = db.prepare('DELETE FROM posts WHERE author = ?');
  final deletePersonTags = db.prepare(
    'DELETE FROM person_tags WHERE person = ?',
  );
  final deletePerson = db.prepare('DELETE FROM people WHERE id = ?');

  rows.all(ComplexRows.delete, Runs.deletedCount, (d) {
    db.execute('BEGIN');

    for (var round = 0; round < Runs.deletedCount; round++) {
      final id = Runs.deleted(round);

      deletePostTags.execute([id]);
      deletePosts.execute([id]);

      final deleted = db.updatedRows;

      deletePersonTags.execute([id]);
      deletePerson.execute([id]);

      if (db.updatedRows == 1) {
        d.add(id, deleted);
      }
    }

    db.execute('COMMIT');
  });

  final leftPeople = Digest();
  final leftPosts = Digest();

  _people(db.prepare('$_person ORDER BY id'), const [], leftPeople);
  _posts(db.prepare('$_post ORDER BY id'), const [], leftPosts);
  rows.check('left-people', leftPeople);
  rows.check('left-posts', leftPosts);
  db.close();
}
