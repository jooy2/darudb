// SQLite through better-sqlite3, which bundles SQLite. A write-ahead log,
// with synchronous = FULL for the sync commits and NORMAL for the deferred
// ones; fullfsync is on, so that on Apple systems SQLite flushes the way
// DaruDB does, and it changes nothing elsewhere. The page cache is 32 MiB,
// DaruDB's default. Reads run in one transaction through prepared statements.
import Sqlite from 'better-sqlite3';
import { join } from 'node:path';

import { Digest, OBJECTS, Rows, finish, person, randomIds, randomNumbers } from './common.mjs';

const [, , directory] = process.argv;
const COLUMNS = 'id, name, email, age, city, score';
const INSERT =
  'INSERT INTO people (name, email, age, city, score) VALUES (@name, @email, @age, @city, @score)';

const open = (path) => {
  const db = new Sqlite(path);

  db.pragma('journal_mode = WAL');
  db.pragma('synchronous = FULL');
  db.pragma('fullfsync = ON');
  db.pragma('cache_size = -32768');
  db.exec(`CREATE TABLE IF NOT EXISTS people (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
    age INTEGER NOT NULL, city TEXT NOT NULL, score REAL NOT NULL);
    CREATE INDEX IF NOT EXISTS people_age ON people (age DESC);`);

  return db;
};

const rows = new Rows();
let db = open(join(directory, 'commits.sqlite'));
let insert = db.prepare(INSERT);
const one = db.transaction((p) => insert.run(p));

rows.each('insert-sync', 500, (round) => one(person(round)));
db.pragma('synchronous = NORMAL');
rows.each('insert-deferred', 10_000, (round) => one(person(1_000 + round)));
db.close();

db = open(join(directory, 'objects.sqlite'));
insert = db.prepare(INSERT);

rows.all('insert-bulk', OBJECTS, () => {
  db.exec('BEGIN');

  for (let n = 0; n < OBJECTS; n++) {
    insert.run(person(n));
  }

  db.exec('COMMIT');
});

const ids = randomIds(OBJECTS);
const emails = randomNumbers(20_000).map((n) => `${n}@example.com`);
const byId = db.prepare(`SELECT ${COLUMNS} FROM people WHERE id = ?`);
const byEmail = db.prepare(`SELECT ${COLUMNS} FROM people WHERE email = ?`);
const byAge = db.prepare(`SELECT ${COLUMNS} FROM people WHERE age = ?`);
const range = db.prepare(
  `SELECT ${COLUMNS} FROM people WHERE age BETWEEN ? AND ? ORDER BY age DESC, id ASC LIMIT 20`
);
const atLeast = db.prepare('SELECT count(*) AS n FROM people WHERE age >= ?');
const inCity = db.prepare(`SELECT ${COLUMNS} FROM people WHERE city = ?`);
const top = db.prepare(`SELECT ${COLUMNS} FROM people ORDER BY score DESC, id ASC LIMIT 10`);

db.exec('BEGIN');
rows.each('get-key', OBJECTS, (round, d) => {
  const found = byId.get(ids[round]);

  if (found) d.person(found);
});
rows.each('get-email', 20_000, (round, d) => {
  const found = byEmail.get(emails[round]);

  if (found) d.person(found);
});
rows.each('age-equal', 200, (round, d) => {
  for (const found of byAge.all(round % 80)) d.person(found);
});
rows.each('age-range', 5_000, (round, d) => {
  const age = round % 76;

  for (const found of range.all(age, age + 4)) d.person(found);
});
rows.each('count', 200, (_, d) => d.number(atLeast.get(40).n));
rows.each('city-scan', 10, (round, d) => {
  for (const found of inCity.all(`city ${round % 100}`)) d.person(found);
});
rows.each('top-score', 10, (_, d) => {
  for (const found of top.all()) d.person(found);
});
db.exec('COMMIT');

const setAge = db.prepare('UPDATE people SET age = ? WHERE id = ?');
const remove = db.prepare('DELETE FROM people WHERE id = ?');

rows.all('update', 10_000, () => {
  db.exec('BEGIN');

  for (let round = 0; round < 10_000; round++) {
    const id = ids[round];
    const found = byId.get(id);

    if (found) {
      found.age = (id + 1) % 80;
      setAge.run(found.age, id);
    }
  }

  db.exec('COMMIT');
});
rows.all('delete', 10_000, (d) => {
  db.exec('BEGIN');

  for (let round = 0; round < 10_000; round++) {
    if (remove.run(1 + round * 7).changes === 1) d.number(1);
  }

  db.exec('COMMIT');
});

const left = new Digest();

for (const found of db.prepare(`SELECT ${COLUMNS} FROM people ORDER BY id`).iterate()) {
  left.person(found);
}
rows.check('left', left);
db.close();
finish(rows);
