/**
 * Collections of objects through the package: a schema declared in
 * JavaScript, objects written and read as records, queries built and
 * written as text, migrations with JavaScript functions, and the errors each
 * can meet. What the engine decides is tested in Rust; what is checked here
 * is that values, keys, queries and errors cross the language boundary
 * intact.
 */
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { collection, conditions, Database, Query, schema, t } from '../index.js';

/** A path in a directory of the test's own, removed when the test ends. */
const tempPath = (context) => {
  const dir = mkdtempSync(join(tmpdir(), 'darudb-node-'));

  context.after(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  return join(dir, 'app.darudb');
};

/** Asserts that `run` throws an `Error` whose `code` is `code`. */
const assertCode = (run, code) => {
  assert.throws(run, (error) => {
    assert.ok(error instanceof Error, 'a real Error is thrown');
    assert.equal(error.code, code, error.message);

    return true;
  });
};

const teams = collection({ name: t.string().primaryKey(), city: t.string().optional() });

const users = collection({
  name: t.string(),
  email: t.string().optional().unique(),
  age: t.int().default(0).index(),
  score: t.float().optional(),
  tags: t.list(t.string()).optional().index(),
  team: t.link('teams').optional(),
  address: t.object({ city: t.string(), zip: t.int().optional() }).optional(),
  avatar: t.bytes().optional(),
  big: t.int().optional()
});

const v1 = schema(1, { teams, users });

/** A database at `v1` with two teams and three users. */
const withData = (context) => {
  const path = tempPath(context);
  const db = Database.open(path, { schema: v1 });

  context.after(() => db.close());

  db.write((txn) => {
    txn.collection('teams').insertMany([{ name: 'north', city: 'Seoul' }, { name: 'south' }]);
    txn.collection('users').insertMany([
      {
        name: 'Alice',
        email: 'alice@example.com',
        age: 31,
        score: 1.5,
        tags: ['red', 'blue'],
        team: 'north',
        address: { city: 'Seoul' },
        avatar: new Uint8Array([1, 2, 3])
      },
      { name: 'Bob', age: 17, team: 'south' },
      { name: 'Carol', age: 40, score: 2, tags: [], big: 2n ** 60n }
    ]);
  });

  return { path, db };
};

describe('objects', () => {
  it('read back as they were written, with defaults and nulls filled in', (context) => {
    const { db } = withData(context);
    const [alice, bob, carol] = db.read((txn) => txn.collection('users').find());

    assert.deepEqual(alice, {
      id: 1,
      name: 'Alice',
      email: 'alice@example.com',
      age: 31,
      score: 1.5,
      tags: ['red', 'blue'],
      team: 'north',
      address: { city: 'Seoul', zip: null },
      avatar: new Uint8Array([1, 2, 3]),
      big: null
    });
    assert.equal(bob.age, 17);
    assert.equal(bob.email, null);
    assert.equal(bob.tags, null);
    assert.deepEqual(carol.tags, [], 'an empty list is not null');
    assert.equal(carol.score, 2);
    assert.equal(carol.big, 2n ** 60n, 'an int beyond 2^53 reads as a bigint');
    assert.equal(
      db.read((txn) => txn.collection('teams').get('north')).city,
      'Seoul',
      'a string primary key'
    );
    assert.equal(db.schemaVersion, 1);
  });

  it('get their keys from an auto-increment or their own field', (context) => {
    const { db } = withData(context);

    db.write((txn) => {
      const users = txn.collection('users');

      assert.equal(users.insert({ name: 'Dave' }), 4);
      assert.equal(users.insert({ id: 10, name: 'Eve' }), 10);
      assert.equal(users.insert({ name: 'Frank' }), 11);
      assert.equal(users.delete(10), true);
      assert.equal(users.delete(10), false);
      assert.equal(users.put({ id: 2, name: 'Robert', age: 18 }), 2);
      assert.deepEqual(txn.collection('teams').insertMany([{ name: 'east' }]), ['east']);
    });

    assert.equal(db.read((txn) => txn.collection('users').get(2)).name, 'Robert');
    assert.equal(
      db.read((txn) => txn.collection('users').get(99)),
      null
    );
  });

  it('are found by queries built in JavaScript', (context) => {
    const { db } = withData(context);
    const names = (query) =>
      db.read((txn) =>
        txn
          .collection('users')
          .find(query)
          .map((user) => user.name)
      );

    assert.deepEqual(
      names((q) => q.where('age', '>=', 18).sortBy('age', 'desc')),
      ['Carol', 'Alice']
    );
    assert.deepEqual(
      names((q) => q.where('email', '==', null)),
      ['Bob', 'Carol']
    );
    assert.deepEqual(
      names((q) => q.where('tags', 'contains', 'red')),
      ['Alice']
    );
    assert.deepEqual(
      names((q) => q.where('score', '>', 1)),
      ['Alice', 'Carol']
    );
    assert.deepEqual(
      names((q) => q.where('team.city', '==', 'Seoul')),
      ['Alice']
    );
    assert.deepEqual(
      names((q) => q.where('address.city', 'startsWith', 'Se')),
      ['Alice']
    );
    assert.deepEqual(
      names((q) => q.where('age', 'between', [17, 31]).sortBy('name', 'desc')),
      ['Bob', 'Alice']
    );
    assert.deepEqual(
      names((q) => q.where('name', 'in', ['Bob', 'Carol', 'Zed'])),
      ['Bob', 'Carol']
    );
    assert.deepEqual(
      names((q) => q.where((c) => c.or(c.eq('name', 'Bob'), c.not(c.isNull('score'))))),
      ['Alice', 'Bob', 'Carol']
    );
    assert.deepEqual(
      names((q) => q.sortBy('age').offset(1).limit(1)),
      ['Alice']
    );
    assert.deepEqual(
      names(new Query().where(conditions.and(conditions.ge('age', 18), conditions.lt('age', 35)))),
      ['Alice']
    );

    db.read((txn) => {
      const users = txn.collection('users');

      assert.equal(
        users.count((q) => q.where('age', '>', 0)),
        3
      );
      assert.equal(users.count(), 3);
      assert.equal(users.findOne((q) => q.sortBy('age', 'desc')).name, 'Carol');
      assert.equal(
        users.findOne((q) => q.where('name', '==', 'Nobody')),
        null
      );
    });
  });

  it('are found by queries written as text, with parameters', (context) => {
    const { db } = withData(context);

    db.read((txn) => {
      const users = txn.collection('users');

      assert.deepEqual(
        users
          .find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC', [18, 'A'])
          .map((user) => user.name),
        ['Alice']
      );
      assert.equal(users.count('score > 1'), 2);
      assert.equal(users.findOne('tags CONTAINS "blue"').name, 'Alice');
      assert.equal(users.count('email == $0', [null]), 2);
    });
  });

  it("are refused, with the engine's codes, when they do not fit", (context) => {
    const { db } = withData(context);

    db.write((txn) => {
      const users = txn.collection('users');

      assertCode(
        () => users.insert({ name: 'Alice', email: 'alice@example.com' }),
        'DUPLICATE_KEY'
      );
      assertCode(() => users.insert({ id: 1, name: 'Again' }), 'DUPLICATE_KEY');
      assertCode(() => users.insert({ age: 3 }), 'INVALID_ARGUMENT');
      assertCode(() => users.insert({ name: 3 }), 'INVALID_ARGUMENT');
      assertCode(() => users.insert({ name: 'x', age: 1.5 }), 'INVALID_ARGUMENT');
      assertCode(() => users.insert({ name: 'x', tags: ['a', null] }), 'INVALID_ARGUMENT');
      assertCode(() => users.insert({ name: 'x', team: 5 }), 'INVALID_ARGUMENT');
      assertCode(() => users.insert('not an object'), 'INVALID_ARGUMENT');
      assertCode(() => users.find((q) => q.where('agee', '>', 1)), 'INVALID_QUERY');
      assertCode(() => users.find((q) => q.where('age', '==', 'x')), 'INVALID_QUERY');
      assertCode(() => users.find((q) => q.where('age', 'like', 1)), 'INVALID_QUERY');
      assertCode(() => users.find('age >'), 'INVALID_QUERY');
      assertCode(() => users.get(1.5), 'INVALID_ARGUMENT');
      assertCode(() => txn.collection('orders'), 'INVALID_ARGUMENT');

      // A refused write leaves the transaction able to commit.
      users.insert({ name: 'Dave' });
    });

    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      4
    );
  });
});

describe('transactions', () => {
  it('commit when their function returns and abort when it throws', (context) => {
    const { db } = withData(context);

    assert.throws(
      () =>
        db.write((txn) => {
          txn.collection('users').insert({ name: 'Ghost' });
          throw new Error('changed my mind');
        }),
      /changed my mind/
    );
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      3
    );
    assert.equal(
      db.write((txn) => txn.collection('users').insert({ name: 'Dave' }), {
        durability: 'deferred'
      }),
      4
    );
    db.sync();
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      4
    );
  });

  it('refuse a function that returns a promise, and abort it', (context) => {
    const { db } = withData(context);

    assertCode(
      () =>
        db.write(async (txn) => {
          txn.collection('users').insert({ name: 'Ghost' });
        }),
      'INVALID_ARGUMENT'
    );
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      3
    );
  });

  it('cannot be used once their function has returned', (context) => {
    const { db } = withData(context);
    const users = db.read((txn) => txn.collection('users'));

    assertCode(() => users.get(1), 'CLOSED');
  });

  it('need a schema for collections', (context) => {
    const db = Database.open(tempPath(context));

    context.after(() => db.close());
    assert.equal(db.schemaVersion, null);
    assertCode(() => db.read((txn) => txn.collection('users')), 'INVALID_ARGUMENT');
  });
});

describe('schemas', () => {
  it('are checked every time the file opens', (context) => {
    const { path, db } = withData(context);

    db.close();
    Database.open(path, { schema: v1 }).close();
    assertCode(
      () =>
        Database.open(path, {
          schema: schema(1, { teams, users: collection({ name: t.string() }) })
        }),
      'SCHEMA_MISMATCH'
    );
    Database.open(path, {
      schema: schema(2, {
        teams,
        users: collection({ ...users.fields, nick: t.string().optional() })
      })
    }).close();
    assertCode(() => Database.open(path, { schema: v1 }), 'SCHEMA_TOO_NEW');
  });

  it('refuse a declaration the engine cannot store', (context) => {
    const path = tempPath(context);

    assertCode(
      () => Database.open(path, { schema: schema(1, { a: collection({ b: t.link('nowhere') }) }) }),
      'INVALID_ARGUMENT'
    );
    assertCode(
      () =>
        Database.open(path, {
          schema: schema(1, { a: collection({ b: t.int().primaryKey(), c: t.int().primaryKey() }) })
        }),
      'INVALID_ARGUMENT'
    );
    assertCode(() => schema(0, {}), 'INVALID_ARGUMENT');
    assertCode(() => collection({ a: 'string' }), 'INVALID_ARGUMENT');
  });
});

describe('migrations', () => {
  const v2 = schema(2, {
    teams,
    people: collection({
      fullName: t.string(),
      email: t.string().optional().unique(),
      age: t.string().default(''),
      tags: t.list(t.string()).optional()
    })
  });
  const steps = (run) => [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      run
    }
  ];

  it('run JavaScript functions with the objects as they were', (context) => {
    const { path, db } = withData(context);
    const seen = [];

    db.close();

    const migrated = Database.open(path, {
      schema: v2,
      migrations: steps((m) => {
        seen.push([m.previousVersion, m.version]);

        const people = m.collection('people');

        for (const key of m.previousKeys('users')) {
          const before = m.previous('users', key);

          people.put({ ...people.get(key), age: `${before.age} years` });
        }
      })
    });

    context.after(() => migrated.close());
    assert.deepEqual(seen, [[1, 2]]);
    assert.equal(migrated.schemaVersion, 2);
    assert.deepEqual(
      migrated.read((txn) =>
        txn
          .collection('people')
          .find()
          .map((person) => [person.fullName, person.age])
      ),
      [
        ['Alice', '31 years'],
        ['Bob', '17 years'],
        ['Carol', '40 years']
      ]
    );
  });

  it('leave the file as it was when a function throws', (context) => {
    const { path, db } = withData(context);

    db.close();
    assert.throws(
      () =>
        Database.open(path, {
          schema: v2,
          migrations: steps((m) => {
            m.collection('people').delete(1);
            throw new Error('not today');
          })
        }),
      /not today/
    );

    const again = Database.open(path, { schema: v1 });

    context.after(() => again.close());
    assert.equal(
      again.read((txn) => txn.collection('users').count()),
      3
    );
  });
});
