// Realm through its JavaScript SDK, for applications that move from it: its
// vendor deprecated it in September 2024 and ended support in September
// 2025. Every commit syncs, as Realm offers no commit that leaves the sync
// for later, so the deferred row has no Realm cell. Realm assigns no keys, so
// the application gives each object the key the other stores assign, and it
// has no unique index besides the primary key, so the email is an index.
// Queries are Realm Query Language strings, which Realm parses on every
// call, since it has no prepared queries; sorting by `id` gives the order
// the other stores' results come in. An update sets the one property.
import Realm from 'realm';
import { join } from 'node:path';

import {
  Digest,
  OBJECTS,
  Rows,
  finish,
  person,
  plain,
  randomIds,
  randomNumbers
} from './common.mjs';

const [, , directory] = process.argv;
const PERSON = {
  name: 'Person',
  primaryKey: 'id',
  properties: {
    id: 'int',
    name: 'string',
    email: { type: 'string', indexed: true },
    age: { type: 'int', indexed: true },
    city: 'string',
    score: 'double'
  }
};
const open = (name) => new Realm({ path: join(directory, name), schema: [PERSON] });
const rows = new Rows();
let realm = open('commits.realm');

rows.each('insert-sync', 500, (round) => {
  realm.write(() => realm.create('Person', { id: round + 1, ...person(round) }));
});
rows.none('insert-deferred');
realm.close();

realm = open('objects.realm');

rows.all('insert-bulk', OBJECTS, () => {
  realm.write(() => {
    for (let n = 0; n < OBJECTS; n++) {
      realm.create('Person', { id: n + 1, ...person(n) });
    }
  });
});

const ids = randomIds(OBJECTS);
const emails = randomNumbers(20_000).map((n) => `${n}@example.com`);
const people = realm.objects('Person');
const each = (results, d) => {
  for (const found of results) d.person(plain(found.id, found));
};

rows.each('get-key', OBJECTS, (round, d) => {
  const found = realm.objectForPrimaryKey('Person', ids[round]);

  if (found) d.person(plain(found.id, found));
});
rows.each('get-email', 20_000, (round, d) => {
  const found = people.filtered('email == $0', emails[round])[0];

  if (found) d.person(plain(found.id, found));
});
rows.each('age-equal', 200, (round, d) => {
  each(people.filtered('age == $0 SORT(id ASC)', round % 80), d);
});
rows.each('age-range', 5_000, (round, d) => {
  const age = round % 76;

  each(people.filtered('age BETWEEN {$0, $1} SORT(age DESC, id ASC) LIMIT(20)', age, age + 4), d);
});
rows.each('count', 200, (_, d) => d.number(people.filtered('age >= $0', 40).length));
rows.each('city-scan', 10, (round, d) => {
  each(people.filtered('city == $0 SORT(id ASC)', `city ${round % 100}`), d);
});
rows.each('top-score', 10, (_, d) => {
  each(people.filtered('TRUEPREDICATE SORT(score DESC, id ASC) LIMIT(10)'), d);
});

rows.all('update', 10_000, () => {
  realm.write(() => {
    for (let round = 0; round < 10_000; round++) {
      const id = ids[round];
      const found = realm.objectForPrimaryKey('Person', id);

      if (found) found.age = (id + 1) % 80;
    }
  });
});
rows.all('delete', 10_000, (d) => {
  realm.write(() => {
    for (let round = 0; round < 10_000; round++) {
      const found = realm.objectForPrimaryKey('Person', 1 + round * 7);

      if (found) {
        realm.delete(found);
        d.number(1);
      }
    }
  });
});

const left = new Digest();

each(people.sorted('id'), left);
rows.check('left', left);
realm.close();
finish(rows);
