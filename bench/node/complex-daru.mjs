// The complex suite on DaruDB through its Node.js package, as an application
// declares it: a link from each person to an organization and from each post
// to its author, an embedded address, and lists of tags with an index on
// each. Queries that run many times are prepared once, and an update sets the
// one field it changes.
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

import { Digest, Rows, finish } from './common.mjs';
import { ROWS, batches, objects, runs, updatedCount } from './complex.mjs';

const [, , directory, packageDir] = process.argv;
const { collection, Database, schema, t } = await import(
  pathToFileURL(join(packageDir, 'dist/index.js')).href
);

const app = schema(1, {
  organizations: collection({
    code: t.string().primaryKey(),
    name: t.string(),
    kind: t.string().index(),
    country: t.string().index(),
    founded: t.int()
  }),
  people: collection({
    name: t.string(),
    email: t.string().unique(),
    age: t.int().index(),
    country: t.string().index(),
    address: t.object({ city: t.string(), zip: t.string() }),
    score: t.float(),
    active: t.bool(),
    joinedAt: t.int().index(),
    organization: t.link('organizations').index(),
    tags: t.list(t.string()).index()
  }),
  posts: collection({
    author: t.link('people').index(),
    title: t.string(),
    body: t.string(),
    likes: t.int().index(),
    language: t.string().index(),
    createdAt: t.int().index(),
    pinned: t.bool(),
    tags: t.list(t.string()).index()
  })
});

const rows = new Rows();
const db = Database.open(join(directory, 'complex.darudb'), { schema: app });
const made = objects();
const people = made.people.map(({ city, zip, ...person }) => ({
  ...person,
  address: { city, zip }
}));

rows.all(...ROWS.load, () => {
  for (const batch of batches(made.organizations)) {
    db.write((txn) => txn.collection('organizations').insertMany(batch));
  }

  for (const batch of batches(people)) {
    db.write((txn) => txn.collection('people').insertMany(batch));
  }

  for (const batch of batches(made.posts)) {
    db.write((txn) => txn.collection('posts').insertMany(batch));
  }
});

const newest = db.prepare('posts', 'author == $0 SORT BY createdAt DESC LIMIT 5');
const filter = db.prepare(
  'people',
  'age BETWEEN $0 AND $1 AND country == $2 AND active == true SORT BY score DESC LIMIT 50'
);
const either = db.prepare('people', 'country IN [$0, $1] OR age >= 78');
const prefix = db.prepare('people', 'name STARTSWITH $0');
const authorCountry = db.prepare(
  'posts',
  'author.country == $0 AND likes >= 900 SORT BY createdAt DESC LIMIT 20'
);
const twoLinks = db.prepare('posts', 'author.organization.kind == $0 AND language == $1');
const tag = db.prepare('posts', 'tags CONTAINS $0 SORT BY likes DESC LIMIT 20');
const text = db.prepare('posts', 'body CONTAINS $0');
const likes = db.prepare('posts', 'likes >= 500 AND language == $0');
const inCountry = db.prepare('people', 'country == $0');
const byAuthor = db.prepare('posts', 'author == $0');

const person = (d, p) => d.add(p.id, p.age);
const post = (d, p) => d.add(p.id, p.likes);

db.read((txn) => {
  const organizations = txn.collection('organizations');
  const people = txn.collection('people');
  const posts = txn.collection('posts');

  rows.each(...ROWS.detail, (round, d) => {
    const found = people.get(runs.detail(round));

    if (!found) return;

    person(d, found);

    const organization = organizations.get(found.organization);

    if (organization) d.add(organization.founded, 0);

    for (const p of posts.find(newest, [found.id])) post(d, p);
  });
  rows.each(...ROWS.filter, (round, d) => {
    const [age, country] = runs.filter(round);

    for (const p of people.find(filter, [age, age + 9, country])) person(d, p);
  });
  rows.each(...ROWS.either, (round, d) => d.number(people.count(either, runs.either(round))));
  rows.each(...ROWS.prefix, (round, d) => {
    for (const p of people.find(prefix, [runs.prefix(round)])) person(d, p);
  });
  rows.each(...ROWS.page, (round, d) => {
    const page = people.find((q) => q.sortBy('joinedAt').offset(runs.offset(round)).limit(50));

    for (const p of page) person(d, p);
  });
  rows.each(...ROWS.authorCountry, (round, d) => {
    for (const p of posts.find(authorCountry, [runs.authorCountry(round)])) post(d, p);
  });
  rows.each(...ROWS.twoLinks, (round, d) => d.number(posts.count(twoLinks, runs.twoLinks(round))));
  rows.each(...ROWS.tag, (round, d) => {
    for (const p of posts.find(tag, [runs.tag(round)])) post(d, p);
  });
  rows.each(...ROWS.text, (round, d) => d.number(posts.count(text, [runs.phrase(round)])));
  rows.each(...ROWS.count, (round, d) => d.number(posts.count(likes, [runs.language(round)])));
});

rows.all(ROWS.update, updatedCount(), (d) => {
  db.write((txn) => {
    const people = txn.collection('people');

    for (const country of runs.updated) {
      const found = people.find(inCountry, [country]);

      d.number(found.length);

      for (const p of found) people.update(p.id, { age: (p.age + 1) % 80 });
    }
  });
});
rows.all(ROWS.delete, runs.deletedCount, (d) => {
  db.write((txn) => {
    const people = txn.collection('people');
    const posts = txn.collection('posts');

    for (let round = 0; round < runs.deletedCount; round++) {
      const id = runs.deleted(round);
      let deleted = 0;

      for (const p of posts.find(byAuthor, [id])) {
        if (posts.delete(p.id)) deleted++;
      }

      if (people.delete(id)) d.add(id, deleted);
    }
  });
});

const leftPeople = new Digest();
const leftPosts = new Digest();

db.read((txn) => {
  for (const p of txn.collection('people').find()) person(leftPeople, p);
  for (const p of txn.collection('posts').find()) post(leftPosts, p);
});
rows.check('left-people', leftPeople);
rows.check('left-posts', leftPosts);
db.close();
finish(rows);
