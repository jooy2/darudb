// The complex suite on SQLite through better-sqlite3, with the settings of the
// simple suite: a write-ahead log with synchronous = FULL, fullfsync, and a
// 32 MiB page cache. The schema is what an SQL application would write: a
// foreign key for each link, the address's fields as columns of their own,
// and a table for each list, with an index on every column the other stores
// index. A row is read with its tags, through a subquery, so that every store
// makes the same objects.
import Sqlite from 'better-sqlite3';
import { join } from 'node:path';

import { Digest, Rows, finish } from './common.mjs';
import { ROWS, batches, objects, runs, updatedCount } from './complex.mjs';

const [, , directory] = process.argv;
const PERSON = `SELECT id, name, email, age, country, city, zip, score, active, joined_at,
  organization, (SELECT group_concat(tag) FROM person_tags WHERE person = people.id) AS tags
  FROM people`;
const POST = `SELECT id, author, title, body, likes, language, created_at, pinned,
  (SELECT group_concat(tag) FROM post_tags WHERE post = posts.id) AS tags FROM posts`;

const db = new Sqlite(join(directory, 'complex.sqlite'));

db.pragma('journal_mode = WAL');
db.pragma('synchronous = FULL');
db.pragma('fullfsync = ON');
db.pragma('cache_size = -32768');
db.exec(`CREATE TABLE organizations (
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
  CREATE INDEX post_tags_tag ON post_tags (tag, post);`);

const tags = (text) => (text === null ? [] : text.split(','));

/** The objects the other stores make, from a row of `PERSON`. */
const personOf = (row) => ({
  id: row.id,
  name: row.name,
  email: row.email,
  age: row.age,
  country: row.country,
  address: { city: row.city, zip: row.zip },
  score: row.score,
  active: row.active === 1,
  joinedAt: row.joined_at,
  organization: row.organization,
  tags: tags(row.tags)
});

const postOf = (row) => ({
  id: row.id,
  author: row.author,
  title: row.title,
  body: row.body,
  likes: row.likes,
  language: row.language,
  createdAt: row.created_at,
  pinned: row.pinned === 1,
  tags: tags(row.tags)
});

const person = (d, p) => d.add(p.id, p.age);
const post = (d, p) => d.add(p.id, p.likes);
const rows = new Rows();
const made = objects();
const insertOrganization = db.prepare(
  'INSERT INTO organizations (code, name, kind, country, founded) VALUES (?, ?, ?, ?, ?)'
);
const insertPerson = db.prepare(
  `INSERT INTO people (name, email, age, country, city, zip, score, active, joined_at,
   organization) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
);
const insertPersonTag = db.prepare('INSERT INTO person_tags (person, tag) VALUES (?, ?)');
const insertPost = db.prepare(
  `INSERT INTO posts (author, title, body, likes, language, created_at, pinned)
   VALUES (?, ?, ?, ?, ?, ?, ?)`
);
const insertPostTag = db.prepare('INSERT INTO post_tags (post, tag) VALUES (?, ?)');

rows.all(...ROWS.load, () => {
  for (const batch of batches(made.organizations)) {
    db.transaction(() => {
      for (const o of batch) {
        insertOrganization.run(o.code, o.name, o.kind, o.country, o.founded);
      }
    })();
  }

  for (const batch of batches(made.people)) {
    db.transaction(() => {
      for (const p of batch) {
        const { lastInsertRowid } = insertPerson.run(
          p.name,
          p.email,
          p.age,
          p.country,
          p.city,
          p.zip,
          p.score,
          p.active ? 1 : 0,
          p.joinedAt,
          p.organization
        );

        for (const t of p.tags) insertPersonTag.run(lastInsertRowid, t);
      }
    })();
  }

  for (const batch of batches(made.posts)) {
    db.transaction(() => {
      for (const p of batch) {
        const { lastInsertRowid } = insertPost.run(
          p.author,
          p.title,
          p.body,
          p.likes,
          p.language,
          p.createdAt,
          p.pinned ? 1 : 0
        );

        for (const t of p.tags) insertPostTag.run(lastInsertRowid, t);
      }
    })();
  }
});

const byId = db.prepare(`${PERSON} WHERE id = ?`);
const organization = db.prepare(
  'SELECT code, name, kind, country, founded FROM organizations WHERE code = ?'
);
const newest = db.prepare(`${POST} WHERE author = ? ORDER BY created_at DESC, id LIMIT 5`);
const filter = db.prepare(
  `${PERSON} WHERE age BETWEEN ? AND ? AND country = ? AND active = 1
   ORDER BY score DESC, id LIMIT 50`
);
const either = db
  .prepare('SELECT count(*) FROM people WHERE country IN (?, ?) OR age >= 78')
  .pluck();
const prefix = db.prepare(`${PERSON} WHERE substr(name, 1, length(@p)) = @p ORDER BY id`);
const page = db.prepare(`${PERSON} ORDER BY joined_at, id LIMIT 50 OFFSET ?`);
const authorCountry = db.prepare(
  `${POST} WHERE likes >= 900 AND author IN (SELECT id FROM people WHERE country = ?)
   ORDER BY created_at DESC, id LIMIT 20`
);
const twoLinks = db
  .prepare(
    `SELECT count(*) FROM posts JOIN people ON people.id = posts.author
     JOIN organizations ON organizations.code = people.organization
     WHERE organizations.kind = ? AND posts.language = ?`
  )
  .pluck();
const tag = db.prepare(
  `${POST} WHERE id IN (SELECT post FROM post_tags WHERE tag = ?)
   ORDER BY likes DESC, id LIMIT 20`
);
const text = db.prepare('SELECT count(*) FROM posts WHERE instr(body, ?) > 0').pluck();
const likes = db
  .prepare('SELECT count(*) FROM posts WHERE likes >= 500 AND language = ?')
  .pluck();

db.transaction(() => {
  rows.each(...ROWS.detail, (round, d) => {
    const row = byId.get(runs.detail(round));

    if (!row) return;

    const found = personOf(row);

    person(d, found);

    const o = organization.get(found.organization);

    if (o) d.add(o.founded, 0);

    for (const p of newest.all(found.id)) post(d, postOf(p));
  });
  rows.each(...ROWS.filter, (round, d) => {
    const [age, country] = runs.filter(round);

    for (const p of filter.all(age, age + 9, country)) person(d, personOf(p));
  });
  rows.each(...ROWS.either, (round, d) => d.number(either.get(...runs.either(round))));
  rows.each(...ROWS.prefix, (round, d) => {
    for (const p of prefix.all({ p: runs.prefix(round) })) person(d, personOf(p));
  });
  rows.each(...ROWS.page, (round, d) => {
    for (const p of page.all(runs.offset(round))) person(d, personOf(p));
  });
  rows.each(...ROWS.authorCountry, (round, d) => {
    for (const p of authorCountry.all(runs.authorCountry(round))) post(d, postOf(p));
  });
  rows.each(...ROWS.twoLinks, (round, d) => d.number(twoLinks.get(...runs.twoLinks(round))));
  rows.each(...ROWS.tag, (round, d) => {
    for (const p of tag.all(runs.tag(round))) post(d, postOf(p));
  });
  rows.each(...ROWS.text, (round, d) => d.number(text.get(runs.phrase(round))));
  rows.each(...ROWS.count, (round, d) => d.number(likes.get(runs.language(round))));
})();

const update = db.prepare('UPDATE people SET age = (age + 1) % 80 WHERE country = ?');

rows.all(ROWS.update, updatedCount(), (d) => {
  db.transaction(() => {
    for (const country of runs.updated) d.number(update.run(country).changes);
  })();
});

const deletePostTags = db.prepare(
  'DELETE FROM post_tags WHERE post IN (SELECT id FROM posts WHERE author = ?)'
);
const deletePosts = db.prepare('DELETE FROM posts WHERE author = ?');
const deletePersonTags = db.prepare('DELETE FROM person_tags WHERE person = ?');
const deletePerson = db.prepare('DELETE FROM people WHERE id = ?');

rows.all(ROWS.delete, runs.deletedCount, (d) => {
  db.transaction(() => {
    for (let round = 0; round < runs.deletedCount; round++) {
      const id = runs.deleted(round);

      deletePostTags.run(id);

      const deleted = deletePosts.run(id).changes;

      deletePersonTags.run(id);

      if (deletePerson.run(id).changes === 1) d.add(id, deleted);
    }
  })();
});

const leftPeople = new Digest();
const leftPosts = new Digest();

for (const p of db.prepare(`${PERSON} ORDER BY id`).iterate()) person(leftPeople, personOf(p));
for (const p of db.prepare(`${POST} ORDER BY id`).iterate()) post(leftPosts, postOf(p));
rows.check('left-people', leftPeople);
rows.check('left-posts', leftPosts);
db.close();
finish(rows);
