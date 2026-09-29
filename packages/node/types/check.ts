/**
 * What the declared types promise, checked by `tsc -p types` in `npm test`:
 * a schema gives each collection's objects their type, the builder refuses a
 * field the collection lacks or a value of another type, and each line below
 * marked `@ts-expect-error` stays an error. Nothing here runs.
 */
import { collection, conditions, Database, param, Query, schema, t } from '../dist/index.js';
import type { CheckReport, Key, SalvageReport } from '../dist/index.js';

const app = schema(1, {
  teams: collection({ name: t.string().primaryKey(), city: t.string().optional() }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index(),
    tags: t.list(t.string()).optional(),
    team: t.link('teams').optional(),
    friends: t.list(t.link('users')).optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional(),
    visits: t.bigint().default(0n)
  })
});

// What the engine refuses in a declaration is refused here too.
// @ts-expect-error a link has no default.
t.link('teams').default('north');
// @ts-expect-error a float cannot be a primary key.
t.float().primaryKey();
// @ts-expect-error a primary key is required.
t.string().optional().primaryKey();
// @ts-expect-error a primary key is required.
t.string().primaryKey().optional();
// @ts-expect-error an embedded object has no index.
t.object({ a: t.int() }).index();

declare const db: Database<typeof app>;

db.write((txn) => {
  const users = txn.collection('users');
  const key: Key = users.insert({ name: 'Alice' });

  users.insertMany([{ name: 'Bob', age: 17, tags: ['a'], address: { city: 'Seoul' } }]);
  users.put({ id: 1, name: 'Alice', email: null });

  // @ts-expect-error `name` is required and has no default.
  users.insert({ age: 3 });
  // @ts-expect-error `age` is an int.
  users.insert({ name: 'Carol', age: 'old' });
  // @ts-expect-error the schema has no such collection.
  txn.collection('orders');

  const teams = txn.collection('teams');

  teams.insert({ name: 'north' });
  // @ts-expect-error a collection with a primary key has no `id`.
  teams.insert({ name: 'south', id: 2 });

  // An update names any fields, and null where a put takes null.
  const updated: boolean = users.update(1, { age: 18, email: null, address: { city: 'Busan' } });

  const alice = users.get(1);

  // An object as it was read, spread, is a set of changes too.
  if (alice !== null) {
    users.update(key, { ...alice, age: alice.age + 1 });
  }

  users.update(1, { age: null, visits: 3n });
  teams.update('north', { city: null });
  // @ts-expect-error `name` is required and has no default, so it is never null.
  users.update(1, { name: null });
  // @ts-expect-error `age` is an int.
  users.update(1, { age: 'old' });
  // @ts-expect-error the collection has no such field.
  users.update(1, { nickname: 'Al' });

  return updated && key;
});

const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);
const first = adults[0];
const age: number = first.age;
const email: string | null = first.email;
const id: number = first.id;
const city: string | undefined = first.address?.city;
const visits: bigint = first.visits;
const friends: Key[] | null = first.friends;
// @ts-expect-error an embedded object has no `id`.
const embeddedId = first.address?.id;

db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null).where('tags', 'contains', 'red'));
  users.find((q) => q.where('address.city', '==', 'Seoul').where('age', 'between', [18, 30]));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
  users.find('age >= $0 SORT BY age DESC', [18]);
  users.count((q) => q.where('name', 'startsWith', 'A'));
  users.findOne(new Query<typeof first>().where('age', '>', 1));
  // @ts-expect-error `agee` is not a field.
  users.find((q) => q.where('agee', '>=', 18));
  // @ts-expect-error `age` holds numbers.
  users.find((q) => q.where('age', '==', 'eighteen'));
  // @ts-expect-error a sort is `asc` or `desc`.
  users.find((q) => q.sortBy('age', 'down'));
});

new Query().where(conditions.not(conditions.eq('a', 1))).limit(3);

// A prepared query finds the objects of the collection it was prepared on.
const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)).sortBy('age'));
const inRange = db.prepare('users', (q) =>
  q.where('age', 'between', [param(0), 30]).where((c) => c.in('name', [param(1), 'Bob']))
);
const byName = db.prepare('teams', 'name == $0');
const preparedCount: number = db.read((txn) => {
  const users = txn.collection('users');
  const found: typeof first | null = users.findOne(byEmail, ['alice@example.com']);
  const inTeam: string | null | undefined = txn
    .collection('teams')
    .find(byName, ['north'])[0]?.city;

  users.find(inRange, [18, 'Alice']);
  // @ts-expect-error a query prepared on `teams` finds teams.
  users.find(byName, ['north']);
  // @ts-expect-error a parameter is a single value.
  users.find(byEmail, [{}]);
  // @ts-expect-error `age` holds numbers, and a parameter does not change that.
  db.prepare('users', (q) => q.where('age', '>=', 'old').where('name', '==', param(0)));
  // @ts-expect-error the schema has no such collection.
  db.prepare('orders', 'name == $0');

  return found === null || inTeam === undefined ? 0 : users.count(byEmail, [null]);
});

const plain = Database.open('plain.darudb');
const version: number | null = plain.schemaVersion;

// The asynchronous API has the same types, resolved.
async function asynchronous() {
  const opened = await Database.openAsync('app.darudb', {
    schema: app,
    migrations: [
      {
        version: 1,
        async run(m) {
          const users = m.collection('users');

          for (const key of await m.previousKeys('users')) {
            const before = await m.previous('users', key);

            await users.put({ id: key as number, name: String(before?.name) });
          }
        }
      }
    ]
  });
  const key: Key = await opened.writeAsync(async (txn) => {
    const users = txn.collection('users');

    // @ts-expect-error `age` is an int.
    await users.insert({ name: 'Carol', age: 'old' });
    // @ts-expect-error the schema has no such collection.
    txn.collection('orders');

    return users.insert({ name: 'Alice' });
  });
  const found = await opened.readAsync((txn) =>
    txn.collection('users').find((q) => q.where('age', '>=', 18))
  );
  const total: number = await opened.readAsync(async (txn) => txn.collection('users').count());
  const prepared = await opened.readAsync((txn) =>
    txn.collection('users').find(byEmail, ['alice@example.com'])
  );
  const one = await opened.readAsync((txn) => txn.collection('users').get(key));
  // @ts-expect-error `get` may find nothing.
  const oneAge: number = one.age;
  // @ts-expect-error a read transaction does not write.
  await opened.readAsync((txn) => txn.collection('users').insert({ name: 'Eve' }));
  Database.open('app.darudb', {
    schema: app,
    migrations: [
      // @ts-expect-error a migration function of `open` gets the synchronous API.
      { version: 1, run: (m) => m.previous('users', 1).then(() => {}) }
    ]
  });

  await opened.syncAsync();
  await opened.closeAsync();

  return [found[0].visits, total, oneAge, prepared[0]?.age] as const;
}

// The integrity check reports rather than throws.
const report: CheckReport = db.check();
const firstPage: number | null = report.problems[0]?.page ?? null;
const checkedLater: Promise<CheckReport> = db.checkAsync();

// An encrypted database opens with a key of bytes or a password.
const keyed = Database.open('secret.darudb', { key: new Uint8Array(32) });
const encrypted: boolean = keyed.isEncrypted;
keyed.setPassword('a new password');
Database.open('secret.darudb', {
  password: new TextEncoder().encode('bytes'),
  passwordHashing: { memoryKib: 19456, iterations: 2, parallelism: 1 }
});
// @ts-expect-error a key is bytes, not a string.
Database.open('secret.darudb', { key: 'a string' });

// Salvage belongs to no open database, and may find no commit to start from.
const salvaged: SalvageReport = Database.salvage('damaged.darudb', 'rescued.darudb');
const salvagedFrom: number | null = salvaged.commitId;
const salvagedLater: Promise<SalvageReport> = Database.salvageAsync('a', 'b', { busyTimeout: 10 });
// @ts-expect-error salvage takes no schema: the file's own comes with it.
Database.salvage('damaged.darudb', 'rescued.darudb', { schema: app });

export {
  app,
  age,
  email,
  id,
  city,
  visits,
  friends,
  embeddedId,
  version,
  preparedCount,
  asynchronous,
  firstPage,
  checkedLater,
  salvagedFrom,
  salvagedLater,
  encrypted
};
