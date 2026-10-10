/// SQLite through the `sqlite3` package, whose build hook provides SQLite. A
/// write-ahead log, with `synchronous = FULL` for the sync commits and
/// `NORMAL` for the deferred ones; `fullfsync` is on, so that on Apple
/// systems SQLite flushes the way DaruDB does, and it changes nothing
/// elsewhere. The page cache is 32 MiB, DaruDB's default. Reads run in one
/// transaction through prepared statements, and each row is made into an
/// object.
library;

import 'package:sqlite3/sqlite3.dart';

import 'common.dart';

const _columns = 'id, name, email, age, city, score';
const _insert =
    'INSERT INTO people (name, email, age, city, score) VALUES (?, ?, ?, ?, ?)';

Database _open(String path) {
  final db = sqlite3.open(path);

  db.execute('''PRAGMA journal_mode = WAL;
    PRAGMA synchronous = FULL;
    PRAGMA fullfsync = ON;
    PRAGMA cache_size = -32768;
    CREATE TABLE IF NOT EXISTS people (
      id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
      age INTEGER NOT NULL, city TEXT NOT NULL, score REAL NOT NULL);
    CREATE INDEX IF NOT EXISTS people_age ON people (age DESC);''');

  return db;
}

Fields _row(Row row) => Fields(
  row.columnAt(0) as int,
  row.columnAt(1) as String,
  row.columnAt(2) as String,
  row.columnAt(3) as int,
  row.columnAt(4) as String,
  row.columnAt(5) as double,
);

void _each(PreparedStatement statement, List<Object?> parameters, Digest d) {
  for (final row in statement.select(parameters)) {
    final p = _row(row);

    d.add(p.id, p.age);
  }
}

List<Object> _values(Fields p) => [p.name, p.email, p.age, p.city, p.score];

void run(String directory, Rows rows) {
  var db = _open('$directory/commits.sqlite');
  var insert = db.prepare(_insert);

  void one(Fields p) {
    db.execute('BEGIN');
    insert.execute(_values(p));
    db.execute('COMMIT');
  }

  rows.each('insert-sync', 500, (round, _) => one(person(round)));
  db.execute('PRAGMA synchronous = NORMAL');
  rows.each('insert-deferred', 10000, (round, _) => one(person(1000 + round)));
  insert.close();
  db.close();

  db = _open('$directory/objects.sqlite');
  insert = db.prepare(_insert);

  rows.all('insert-bulk', objects, (_) {
    db.execute('BEGIN');

    for (var n = 0; n < objects; n++) {
      insert.execute(_values(person(n)));
    }

    db.execute('COMMIT');
  });

  final ids = randomIds(objects);
  final emails = [for (final n in randomNumbers(20000)) '$n@example.com'];
  final byId = db.prepare('SELECT $_columns FROM people WHERE id = ?');
  final byEmail = db.prepare('SELECT $_columns FROM people WHERE email = ?');
  final byAge = db.prepare('SELECT $_columns FROM people WHERE age = ?');
  final range = db.prepare(
    'SELECT $_columns FROM people WHERE age BETWEEN ? AND ? '
    'ORDER BY age DESC, id ASC LIMIT 20',
  );
  final atLeast = db.prepare('SELECT count(*) FROM people WHERE age >= ?');
  final inCity = db.prepare('SELECT $_columns FROM people WHERE city = ?');
  final top = db.prepare(
    'SELECT $_columns FROM people ORDER BY score DESC, id ASC LIMIT 10',
  );

  db.execute('BEGIN');
  rows.each('get-key', objects, (round, d) => _each(byId, [ids[round]], d));
  rows.each(
    'get-email',
    20000,
    (round, d) => _each(byEmail, [emails[round]], d),
  );
  rows.each('age-equal', 200, (round, d) => _each(byAge, [round % 80], d));
  rows.each('age-range', 5000, (round, d) {
    final age = round % 76;

    _each(range, [age, age + 4], d);
  });
  rows.each('count', 200, (_, d) {
    d.number(atLeast.select([40]).first.columnAt(0) as int);
  });
  rows.each(
    'city-scan',
    10,
    (round, d) => _each(inCity, ['city ${round % 100}'], d),
  );
  rows.each('top-score', 10, (_, d) => _each(top, const [], d));
  db.execute('COMMIT');

  final setAge = db.prepare('UPDATE people SET age = ? WHERE id = ?');
  final remove = db.prepare('DELETE FROM people WHERE id = ?');

  rows.all('update', 10000, (_) {
    db.execute('BEGIN');

    for (var round = 0; round < 10000; round++) {
      final id = ids[round];
      final found = byId.select([id]);

      if (found.isNotEmpty) {
        final p = _row(found.first)..age = (id + 1) % 80;

        setAge.execute([p.age, id]);
      }
    }

    db.execute('COMMIT');
  });
  rows.all('delete', 10000, (d) {
    db.execute('BEGIN');

    for (var round = 0; round < 10000; round++) {
      remove.execute([1 + round * 7]);

      if (db.updatedRows == 1) d.number(1);
    }

    db.execute('COMMIT');
  });

  final left = Digest();

  _each(db.prepare('SELECT $_columns FROM people ORDER BY id'), const [], left);
  rows.check('left', left);
  db.close();
}
