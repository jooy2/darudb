// LMDB through lmdb-js, whose values are MessagePack, its default. A sync
// commit is a synchronous transaction with the default flush; a deferred one
// runs with noSync, and the environment is flushed once at the end, as
// closing a DaruDB file syncs its deferred commits. overlappingSync is off,
// since with it a commit returns before its flush. The unique and age
// indexes are databases of their own, written by hand as an application
// would.
import { open } from 'lmdb';
import { join } from 'node:path';

import { Digest, OBJECTS, Rows, finish, person, randomIds, randomNumbers } from './common.mjs';

const [, , directory] = process.argv;
const EMPTY = Buffer.alloc(0);

const openStore = (path, noSync) => {
  const root = open({ path, noSync, overlappingSync: false, maxDbs: 4 });
  const people = root.openDB('people', { keyEncoding: 'uint32' });
  const email = root.openDB('email', {});
  const age = root.openDB('age', { encoding: 'binary' });
  const last = people.getKeys({ reverse: true, limit: 1 }).asArray[0];
  let next = (last ?? 0) + 1;

  return {
    root,
    people,
    email,
    age,
    insert(p) {
      const id = next++;

      people.putSync(id, p);
      email.putSync(p.email, id);
      age.putSync([p.age, id], EMPTY);
    },
    get(id) {
      const found = people.get(id);

      if (found !== undefined) found.id = id;
      return found;
    },
    ofAge(a, limit, d) {
      let given = 0;

      for (const key of age.getKeys({ start: [a, 0], end: [a + 1, 0], limit })) {
        d.person(this.get(key[1]));
        given++;
      }
      return given;
    }
  };
};

const rows = new Rows();
let store = openStore(join(directory, 'commits.lmdb'), false);

rows.each('insert-sync', 500, (round) =>
  store.root.transactionSync(() => store.insert(person(round)))
);
await store.root.close();
store = openStore(join(directory, 'commits.lmdb'), true);
rows.each('insert-deferred', 10_000, (round) =>
  store.root.transactionSync(() => store.insert(person(1_000 + round)))
);
await store.root.flushed;
await store.root.close();

store = openStore(join(directory, 'objects.lmdb'), false);

rows.all('insert-bulk', OBJECTS, () => {
  store.root.transactionSync(() => {
    for (let n = 0; n < OBJECTS; n++) {
      store.insert(person(n));
    }
  });
});

const ids = randomIds(OBJECTS);
const emails = randomNumbers(20_000).map((n) => `${n}@example.com`);
const read = store.root.useReadTransaction();

rows.each('get-key', OBJECTS, (round, d) => {
  const found = store.get(ids[round]);

  if (found) d.person(found);
});
rows.each('get-email', 20_000, (round, d) => {
  const id = store.email.get(emails[round]);

  if (id !== undefined) d.person(store.get(id));
});
rows.each('age-equal', 200, (round, d) => store.ofAge(round % 80, undefined, d));
rows.each('age-range', 5_000, (round, d) => {
  const low = round % 76;
  let left = 20;

  for (let a = low + 4; a >= low && left > 0; a--) left -= store.ofAge(a, left, d);
});
rows.each('count', 200, (_, d) => d.number(store.age.getKeysCount({ start: [40, 0] })));
rows.each('city-scan', 10, (round, d) => {
  const city = `city ${round % 100}`;

  for (const { key, value } of store.people.getRange()) {
    if (value.city === city) {
      value.id = key;
      d.person(value);
    }
  }
});
rows.each('top-score', 10, (_, d) => {
  const top = [];

  for (const { key, value } of store.people.getRange()) {
    const score = value.score;

    if (top.length === 10 && !(score > top[9][0] || (score === top[9][0] && key < top[9][1]))) {
      continue;
    }

    let at = top.findIndex(([s, k]) => score > s || (score === s && key < k));

    if (at < 0) at = top.length;
    top.splice(at, 0, [score, key]);
    if (top.length > 10) top.pop();
  }

  for (const [, key] of top) d.person(store.get(key));
});
read.done();

rows.all('update', 10_000, () => {
  store.root.transactionSync(() => {
    for (let round = 0; round < 10_000; round++) {
      const id = ids[round];
      const found = store.people.get(id);

      if (found) {
        store.age.removeSync([found.age, id]);
        found.age = (id + 1) % 80;
        store.people.putSync(id, found);
        store.age.putSync([found.age, id], EMPTY);
      }
    }
  });
});
rows.all('delete', 10_000, (d) => {
  store.root.transactionSync(() => {
    for (let round = 0; round < 10_000; round++) {
      const id = 1 + round * 7;
      const found = store.people.get(id);

      if (found) {
        store.people.removeSync(id);
        store.email.removeSync(found.email);
        store.age.removeSync([found.age, id]);
        d.number(1);
      }
    }
  });
});

const left = new Digest();

for (const { key, value } of store.people.getRange()) {
  value.id = key;
  left.person(value);
}
rows.check('left', left);
await store.root.close();
finish(rows);
