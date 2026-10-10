// DaruDB through its Node.js package, as an application uses it: queries that
// run many times are prepared once, as a statement would be.
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

import { Digest, OBJECTS, Rows, finish, person, randomIds, randomNumbers } from './common.mjs';

const [, , directory, packageDir] = process.argv;
const { collection, Database, schema, t, param } = await import(
  pathToFileURL(join(packageDir, 'dist/index.js')).href
);

const app = schema(1, {
  people: collection({
    name: t.string(),
    email: t.string().unique(),
    age: t.int().index(),
    city: t.string(),
    score: t.float()
  })
});

const rows = new Rows();
let db = Database.open(join(directory, 'commits.darudb'), { schema: app });

rows.each('insert-sync', 500, (round) => {
  db.write((txn) => txn.collection('people').insert(person(round)));
});
rows.each('insert-deferred', 10_000, (round) => {
  db.write((txn) => txn.collection('people').insert(person(1_000 + round)), {
    durability: 'deferred'
  });
});
db.close();

db = Database.open(join(directory, 'objects.darudb'), { schema: app });

rows.all('insert-bulk', OBJECTS, () => {
  db.write((txn) => {
    const people = txn.collection('people');

    for (let n = 0; n < OBJECTS; n++) {
      people.insert(person(n));
    }
  });
});

const ids = randomIds(OBJECTS);
const emails = randomNumbers(20_000).map((n) => `${n}@example.com`);
const byEmail = db.prepare('people', (q) => q.where('email', '==', param(0)));
const byAge = db.prepare('people', (q) => q.where('age', '==', param(0)));
const range = db.prepare('people', (q) =>
  q
    .where('age', 'between', [param(0), param(1)])
    .sortBy('age', 'desc')
    .limit(20)
);
const atLeast = db.prepare('people', (q) => q.where('age', '>=', param(0)));
const inCity = db.prepare('people', (q) => q.where('city', '==', param(0)));
const top = db.prepare('people', (q) => q.sortBy('score', 'desc').limit(10));

db.read((txn) => {
  const people = txn.collection('people');

  rows.each('get-key', OBJECTS, (round, d) => {
    const found = people.get(ids[round]);

    if (found) d.person(found);
  });
  rows.each('get-email', 20_000, (round, d) => {
    const found = people.findOne(byEmail, [emails[round]]);

    if (found) d.person(found);
  });
  rows.each('age-equal', 200, (round, d) => {
    for (const found of people.find(byAge, [round % 80])) d.person(found);
  });
  rows.each('age-range', 5_000, (round, d) => {
    const age = round % 76;

    for (const found of people.find(range, [age, age + 4])) d.person(found);
  });
  rows.each('count', 200, (_, d) => d.number(people.count(atLeast, [40])));
  rows.each('city-scan', 10, (round, d) => {
    for (const found of people.find(inCity, [`city ${round % 100}`])) d.person(found);
  });
  rows.each('top-score', 10, (_, d) => {
    for (const found of people.find(top)) d.person(found);
  });
});

rows.all('update', 10_000, () => {
  db.write((txn) => {
    const people = txn.collection('people');

    for (let round = 0; round < 10_000; round++) {
      const id = ids[round];
      const found = people.get(id);

      if (found) {
        found.age = (id + 1) % 80;
        people.put(found);
      }
    }
  });
});
rows.all('delete', 10_000, (d) => {
  db.write((txn) => {
    const people = txn.collection('people');

    for (let round = 0; round < 10_000; round++) {
      if (people.delete(1 + round * 7)) d.number(1);
    }
  });
});

const left = new Digest();

db.read((txn) => {
  for (const found of txn.collection('people').find()) left.person(found);
});
rows.check('left', left);
db.close();
finish(rows);
