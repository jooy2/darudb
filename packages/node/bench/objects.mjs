/**
 * Measures the package on this machine with the workloads of the engine's
 * `examples/object_bench.rs`, so that the cost of crossing from JavaScript
 * into the engine shows beside what the engine takes on its own.
 *
 *   npm run build && npm run bench [directory]
 *
 * The files go into `directory`, the system's temporary directory by
 * default, and are removed afterwards. The numbers are for comparing builds
 * on one machine; `object_bench.rs` spells out the workloads, which run the
 * same way against other databases outside this repository.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { collection, Database, schema, t } from '../index.js';

/** Objects in the read and query workloads. */
const OBJECTS = 100_000;

const people = schema(1, {
  people: collection({
    name: t.string(),
    email: t.string().unique(),
    age: t.int().index(),
    city: t.string(),
    score: t.float()
  })
});

const person = (n) => ({
  name: `person ${n}`,
  email: `${n}@example.com`,
  age: (n * 7919) % 80,
  city: `city ${n % 100}`,
  score: (n * 0.618) % 1
});

/** Spreads consecutive numbers over the keys, so reads land all over. */
const scatter = (n) => Number((BigInt(n) * 0x9e3779b97f4a7c15n) % BigInt(OBJECTS));

/**
 * Runs `step` `count` times and prints how fast it went, per object when
 * each step handles `objects` of them.
 */
const measure = (name, count, step, objects = 1) => {
  const started = process.hrtime.bigint();

  for (let round = 0; round < count; round++) {
    step(round);
  }

  const nanos = Number(process.hrtime.bigint() - started);
  const each = nanos / (count * objects);
  const text =
    each >= 1e6
      ? `${(each / 1e6).toFixed(2)} ms`
      : each >= 1e3
        ? `${(each / 1e3).toFixed(2)} us`
        : `${Math.round(each)} ns`;

  console.log(
    `${name.padEnd(48)} ${Math.round((count * objects * 1e9) / nanos)
      .toString()
      .padStart(12)} ${text.padStart(14)}`
  );
};

const run = (directory) => {
  console.log(`\n${'plain file'.padEnd(48)} ${'per second'.padStart(12)} ${'each'.padStart(14)}`);

  let db = Database.open(join(directory, 'commits.darudb'), { schema: people });

  measure('insert, one object per sync commit', 500, (round) => {
    db.write((txn) => txn.collection('people').insert(person(round)));
  });
  measure('insert, one object per deferred commit', 10_000, (round) => {
    db.write((txn) => txn.collection('people').insert(person(1_000 + round)), {
      durability: 'deferred'
    });
  });
  db.close();

  db = Database.open(join(directory, 'objects.darudb'), { schema: people });

  const everyone = Array.from({ length: OBJECTS }, (_, n) => person(n));

  measure(
    'insert in one transaction, one call, per object',
    1,
    () => {
      db.write((txn) => txn.collection('people').insertMany(everyone));
    },
    OBJECTS
  );

  db.read((txn) => {
    const people = txn.collection('people');

    measure('get by primary key, random order', OBJECTS, (round) => {
      people.get(1 + scatter(round));
    });
    measure('get by unique email, random order', 20_000, (round) => {
      const email = `${scatter(round)}@example.com`;

      people.findOne((q) => q.where('email', '==', email));
    });
    measure('query age == a, 1250 objects', 200, (round) => {
      people.find((q) => q.where('age', '==', round % 80));
    });
    measure('query age range, sorted descending, limit 20', 5_000, (round) => {
      const age = round % 76;

      people.find((q) =>
        q
          .where('age', 'between', [age, age + 4])
          .sortBy('age', 'desc')
          .limit(20)
      );
    });
    measure('count age >= 40 through the index', 200, () => {
      people.count((q) => q.where('age', '>=', 40));
    });
    measure('query city == c, no index, 1000 objects', 10, (round) => {
      people.find((q) => q.where('city', '==', `city ${round % 100}`));
    });
    measure('top 10 by score, no index', 10, () => {
      people.find((q) => q.sortBy('score', 'desc').limit(10));
    });
    measure('parse and run age == $0 LIMIT 10', 20_000, (round) => {
      people.find('age == $0 LIMIT 10', [round % 80]);
    });
    measure(
      'every object, decoded, per object',
      1,
      () => {
        people.find();
      },
      OBJECTS
    );
  });

  measure(
    'update, get and put, in one transaction',
    1,
    () => {
      db.write((txn) => {
        const people = txn.collection('people');
        const changed = [];

        for (let round = 0; round < 10_000; round++) {
          const found = people.get(1 + scatter(round));

          changed.push({ ...found, age: (found.age + 1) % 80 });
        }

        people.putMany(changed);
      });
    },
    10_000
  );
  measure(
    'delete, in one transaction',
    1,
    () => {
      db.write((txn) => {
        const people = txn.collection('people');

        for (let round = 0; round < 10_000; round++) {
          people.delete(1 + round * 7);
        }
      });
    },
    10_000
  );
  db.close();
};

const directory = mkdtempSync(join(process.argv[2] ?? tmpdir(), 'darudb-bench-'));

try {
  run(directory);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
