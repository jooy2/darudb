/**
 * The asynchronous API: transactions whose operations run on the thread
 * pool, in the order they were called, the queue that keeps this process's
 * writes on one file from waiting for each other there, migrations with
 * asynchronous functions, and the event loop running while the engine waits.
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { describe, it } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

import { collection, Database, param, schema, t } from '../index.js';

/** A path in a directory of the test's own, removed when the test ends. */
const tempPath = (context) => {
  const dir = mkdtempSync(join(tmpdir(), 'darudb-node-'));

  context.after(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  return join(dir, 'app.darudb');
};

/** Asserts that `promise` rejects with an `Error` whose `code` is `code`. */
const assertRejects = (promise, code) =>
  assert.rejects(promise, (error) => {
    assert.ok(error instanceof Error, 'a real Error is thrown');
    assert.equal(error.code, code, error.message);

    return true;
  });

const users = collection({
  name: t.string(),
  email: t.string().optional().unique(),
  age: t.int().default(0).index(),
  avatar: t.bytes().optional(),
  big: t.bigint().optional()
});

const v1 = schema(1, { users });

/** A database at `v1`, opened asynchronously, with three users. */
const withData = async (context) => {
  const path = tempPath(context);
  const db = await Database.openAsync(path, { schema: v1 });

  context.after(() => db.close());
  await db.writeAsync(async (txn) => {
    await txn.collection('users').insertMany([
      { name: 'Alice', email: 'alice@example.com', age: 31, avatar: new Uint8Array([1, 2]) },
      { name: 'Bob', age: 17 },
      { name: 'Carol', age: 40, big: 2n ** 60n }
    ]);
  });

  return { path, db };
};

describe('asynchronous transactions', () => {
  it('read and write what the synchronous API does', async (context) => {
    const { db } = await withData(context);
    const byAge = db.prepare('users', (q) => q.where('age', '>=', param(0)).sortBy('name'));

    const found = await db.readAsync(async (txn) => {
      const users = txn.collection('users');

      return {
        alice: await users.get(1),
        missing: await users.get(9),
        adults: await users.find((q) => q.where('age', '>=', 18).sortBy('age', 'desc')),
        first: await users.findOne('age < $0', [20]),
        count: await users.count((q) => q.where('name', 'startsWith', 'A')),
        prepared: await users.find(byAge, [18]),
        preparedCount: await users.count(byAge, [0])
      };
    });

    assert.deepEqual(found.alice, {
      id: 1,
      name: 'Alice',
      email: 'alice@example.com',
      age: 31,
      avatar: new Uint8Array([1, 2]),
      big: null
    });
    assert.equal(found.missing, null);
    assert.deepEqual(
      found.adults.map((user) => user.name),
      ['Carol', 'Alice']
    );
    assert.equal(found.first.name, 'Bob');
    assert.equal(found.count, 1);
    assert.deepEqual(
      found.prepared.map((user) => user.name),
      ['Alice', 'Carol']
    );
    assert.equal(found.preparedCount, 3);
    assert.deepEqual(
      db.read((txn) => txn.collection('users').find()),
      await db.readAsync((txn) => txn.collection('users').find()),
      'both APIs read the same objects'
    );

    const written = await db.writeAsync(async (txn) => {
      const users = txn.collection('users');

      return [
        await users.put({ id: 2, name: 'Robert', age: 18 }),
        await users.delete(3),
        await users.delete(3),
        await users.insert({ name: 'Dave' })
      ];
    });

    assert.deepEqual(written, [2, true, false, 4]);
    assert.deepEqual(
      db.read((txn) =>
        txn
          .collection('users')
          .find()
          .map((user) => user.name)
      ),
      ['Alice', 'Robert', 'Dave']
    );
  });

  it('run operations in the order they were called, awaited or not', async (context) => {
    const { db } = await withData(context);

    const seen = await db.writeAsync(async (txn) => {
      const users = txn.collection('users');
      const inserted = users.insert({ name: 'Dave' });
      const deleted = users.delete(4);
      const after = users.get(4);

      return [await inserted, await deleted, await after];
    });

    assert.deepEqual(seen, [4, true, null]);

    // Many at once, none awaited: one batch, run in the order called.
    const last = await db.writeAsync((txn) => {
      const users = txn.collection('users');

      for (let n = 0; n < 1000; n++) {
        users.put({ id: 2, name: `Bob ${n}`, age: n });
      }

      return users.get(2);
    });

    assert.equal(last.name, 'Bob 999');

    // Nothing is awaited here: the commit waits for every operation called.
    await db.writeAsync((txn) => {
      txn.collection('users').insertMany([{ name: 'Erin' }, { name: 'Frank' }]);
      txn.collection('users').delete(1);
    });
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      4
    );
  });

  it('give each operation called together its own result, failures included', async (context) => {
    const path = tempPath(context);
    const keyed = schema(1, {
      labels: collection({ label: t.string().primaryKey(), note: t.string().optional() }),
      blobs: collection({ hash: t.bytes().primaryKey() }),
      bigs: collection({ n: t.bigint().primaryKey() })
    });
    const db = await Database.openAsync(path, { schema: keyed });

    context.after(() => db.close());

    // Called in one turn, so they cross into the engine as one batch.
    const settled = await db.writeAsync((txn) => {
      const labels = txn.collection('labels');
      const blobs = txn.collection('blobs');
      const bigs = txn.collection('bigs');

      return Promise.allSettled([
        labels.insert({ label: 'a' }),
        labels.insert({ label: 'a' }),
        labels.put({ label: 'b', note: 'x' }),
        labels.get('b'),
        labels.get('z'),
        labels.count(),
        labels.find((q) => q.sortBy('label', 'desc')),
        labels.findOne('label == $0', ['a']),
        labels.find((q) => q.where('missing', '==', 1)),
        labels.delete('a'),
        labels.delete('a'),
        blobs.insertMany([{ hash: new Uint8Array([1, 2]) }, { hash: new Uint8Array([]) }]),
        bigs.insertMany([{ n: 2n ** 60n }, { n: -5n }]),
        bigs.get(2n ** 60n)
      ]);
    });
    const values = settled.map((outcome) =>
      outcome.status === 'fulfilled' ? outcome.value : outcome.reason.code
    );

    assert.deepEqual(values, [
      'a',
      'DUPLICATE_KEY',
      'b',
      { label: 'b', note: 'x' },
      null,
      2,
      [
        { label: 'b', note: 'x' },
        { label: 'a', note: null }
      ],
      { label: 'a', note: null },
      'INVALID_QUERY',
      true,
      false,
      [Buffer.from([1, 2]), Buffer.from([])],
      [2n ** 60n, -5],
      { n: 2n ** 60n }
    ]);
    assert.ok(settled[1].reason instanceof Error, 'a failure in a batch is a real Error');
    assert.deepEqual(
      db.read((txn) => txn.collection('labels').find()),
      [{ label: 'b', note: 'x' }],
      'the operations after a refused one ran, and committed'
    );
  });

  it('send operations called during a batch after it, in order', async (context) => {
    const { db } = await withData(context);

    const seen = await db.writeAsync(async (txn) => {
      const users = txn.collection('users');

      for (let n = 0; n < 500; n++) {
        users.put({ id: 2, name: `first ${n}` });
      }

      // The first batch is on its way; these queue behind it.
      await Promise.resolve();

      for (let n = 0; n < 500; n++) {
        users.put({ id: 2, name: `second ${n}` });
      }

      return users.get(2);
    });

    assert.equal(seen.name, 'second 499');
  });

  it('commit when their function resolves and abort when it rejects', async (context) => {
    const { db } = await withData(context);

    await assert.rejects(
      db.writeAsync(async (txn) => {
        await txn.collection('users').insert({ name: 'Ghost' });
        throw new Error('changed my mind');
      }),
      /changed my mind/
    );
    await assert.rejects(
      db.writeAsync((txn) => {
        txn.collection('users').insert({ name: 'Ghost' });
        throw new Error('before anything ran');
      }),
      /before anything ran/
    );
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      3
    );
    assert.equal(
      await db.writeAsync((txn) => txn.collection('users').insert({ name: 'Dave' }), {
        durability: 'deferred'
      }),
      4
    );
    await db.syncAsync();
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      4
    );
  });

  it('reject a refused operation with the engine code, and go on after it', async (context) => {
    const { db } = await withData(context);

    await db.writeAsync(async (txn) => {
      const users = txn.collection('users');

      await assertRejects(
        users.insert({ name: 'Copy', email: 'alice@example.com' }),
        'DUPLICATE_KEY'
      );
      await assertRejects(users.insert({ name: 1 }), 'INVALID_ARGUMENT');
      await assertRejects(users.insert({ name: 'Extra', extra: true }), 'INVALID_ARGUMENT');
      await assertRejects(
        users.find((q) => q.where('missing', '==', 1)),
        'INVALID_QUERY'
      );
      await assertRejects(users.insertMany('not an array'), 'INVALID_ARGUMENT');
      await users.insert({ name: 'Dave' });
    });
    await db.readAsync(async (txn) => {
      assert.equal(await txn.collection('users').count(), 4);
      assert.throws(
        () => txn.collection('nothing'),
        (error) => error.code === 'INVALID_ARGUMENT'
      );
    });
  });

  it('end with their function, so a later operation fails with `CLOSED`', async (context) => {
    const { db } = await withData(context);
    let kept;

    await db.writeAsync((txn) => {
      kept = txn.collection('users');
    });
    await assertRejects(kept.get(1), 'CLOSED');
    await assertRejects(kept.insert({ name: 'Late' }), 'CLOSED');
    await db.readAsync((txn) => {
      kept = txn.collection('users');
    });
    await assertRejects(kept.count(), 'CLOSED');
  });

  it('see one commit for as long as their function runs', async (context) => {
    const { db } = await withData(context);

    await db.readAsync(async (txn) => {
      const users = txn.collection('users');

      assert.equal(await users.count(), 3);
      await db.writeAsync((write) => write.collection('users').insert({ name: 'Dave' }));
      assert.equal(await users.count(), 3, 'the read still sees its commit');
    });
    assert.equal(await db.readAsync((txn) => txn.collection('users').count()), 4);
  });

  it('refuse a write on the database after `closeAsync`', async (context) => {
    const { db } = await withData(context);

    await db.closeAsync();
    await db.closeAsync();
    assert.equal(db.isOpen, false);
    await assertRejects(
      db.writeAsync(() => {}),
      'CLOSED'
    );
    await assertRejects(
      db.readAsync(() => {}),
      'CLOSED'
    );
    await assertRejects(db.syncAsync(), 'CLOSED');
  });

  it('refuse a durability they do not know', async (context) => {
    const { db } = await withData(context);

    await assertRejects(
      db.writeAsync(() => {}, { durability: 'eventually' }),
      'INVALID_ARGUMENT'
    );
  });
});

describe('writes on one file', () => {
  it('queue in this process, however many there are and wherever they come from', async (context) => {
    const { path, db } = await withData(context);
    let link = path;

    // A symbolic link needs a privilege on Windows that a test cannot count on.
    if (process.platform !== 'win32') {
      link = join(dirname(path), 'link.darudb');
      symlinkSync(path, link);
    }

    // Two more handles to the same file, one by another name. More writes
    // than the thread pool has threads: waiting there would leave none.
    const other = await Database.openAsync(path, { schema: v1 });
    const linked = await Database.openAsync(link, { schema: v1 });

    context.after(() => {
      other.close();
      linked.close();
    });

    const handles = [db, other, linked];
    const keys = await Promise.all(
      Array.from({ length: 30 }, (_, n) =>
        handles[n % 3].writeAsync(async (txn) => {
          await delay(1);

          return txn.collection('users').insert({ name: `user ${n}` });
        })
      )
    );

    assert.deepEqual(
      [...keys].sort((a, b) => a - b),
      Array.from({ length: 30 }, (_, n) => n + 4)
    );
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      33
    );
  });

  it('do not nest, asynchronous or not', async (context) => {
    const { path, db } = await withData(context);
    const other = await Database.openAsync(path, { schema: v1 });

    context.after(() => other.close());

    await db.writeAsync(async (txn) => {
      await assertRejects(
        db.writeAsync(() => {}),
        'INVALID_ARGUMENT'
      );
      await assertRejects(
        other.writeAsync(() => {}),
        'INVALID_ARGUMENT'
      );
      assert.throws(
        () => other.write(() => {}),
        (error) => error.code === 'INVALID_ARGUMENT'
      );
      await txn.collection('users').insert({ name: 'Dave' });
    });
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      4
    );
  });

  it('refuse a synchronous write while an asynchronous one holds the file', async (context) => {
    const { db } = await withData(context);
    let started;
    const running = new Promise((resolve) => {
      started = resolve;
    });
    let finish;
    const held = db.writeAsync(async (txn) => {
      await txn.collection('users').insert({ name: 'Dave' });
      started();
      await new Promise((resolve) => {
        finish = resolve;
      });
    });

    await running;
    assert.throws(
      () => db.write(() => {}),
      (error) => error.code === 'INVALID_ARGUMENT'
    );
    finish();
    await held;
    db.write((txn) => txn.collection('users').insert({ name: 'Erin' }));
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      5
    );
  });

  it('hold back `sync` and `close`, which may wait for the writer', async (context) => {
    const { path, db } = await withData(context);
    const other = await Database.openAsync(path, { schema: v1 });

    context.after(() => other.close());

    await db.writeAsync(async () => {
      await assertRejects(db.syncAsync(), 'INVALID_ARGUMENT');
      await assertRejects(other.closeAsync(), 'INVALID_ARGUMENT');
      assert.equal(other.isOpen, true, 'a refused close leaves the database open');

      for (const call of [() => other.sync(), () => other.close()]) {
        assert.throws(call, (error) => error.code === 'INVALID_ARGUMENT');
      }

      assert.equal(other.isOpen, true);
    });
    other.sync();
    await other.closeAsync();
    assert.equal(other.isOpen, false);
  });

  it('keep the thread pool free for a write that `syncAsync` calls wait for', async (context) => {
    const path = tempPath(context);
    const db = await Database.openAsync(path, { schema: v1, busyTimeout: 1000 });

    context.after(() => db.close());

    // A deferred commit, which `sync` makes durable under the writer lock.
    await db.writeAsync((txn) => txn.collection('users').insert({ name: 'Alice' }), {
      durability: 'deferred'
    });

    let started;
    const running = new Promise((resolve) => {
      started = resolve;
    });
    let go;
    const writing = db.writeAsync(
      async (txn) => {
        started();
        await new Promise((resolve) => {
          go = resolve;
        });
        await txn.collection('users').insert({ name: 'Bob' });
      },
      { durability: 'deferred' }
    );

    await running;

    // More than the pool has threads. Waiting there for the writer would
    // leave none for the write's own operation.
    const syncs = Array.from({ length: 8 }, () => db.syncAsync());

    await delay(20);
    go();
    await Promise.all([writing, ...syncs]);
    assert.equal(
      db.read((txn) => txn.collection('users').count()),
      2
    );
  });

  it('let a callback the function scheduled write once the function has settled', async (context) => {
    const { db } = await withData(context);
    let later;

    await db.writeAsync(async () => {
      later = new Promise((resolve, reject) => {
        setTimeout(() => {
          db.writeAsync((txn) => txn.collection('users').insert({ name: 'Later' })).then(
            resolve,
            reject
          );
        }, 5);
      });
    });
    assert.equal(await later, 4);
  });

  it("wait for another process's writer on the thread pool, not on the event loop", async (context) => {
    const { path, db } = await withData(context);
    const index = fileURLToPath(new URL('../index.js', import.meta.url));
    const holder = spawn(
      process.execPath,
      [
        '--input-type=module',
        '-e',
        `
          import { createRequire } from 'node:module';
          const { Database } = createRequire(import.meta.url)(${JSON.stringify(index)});
          const db = Database.open(${JSON.stringify(path)});
          db.write(() => {
            process.stdout.write('holding\\n');
            Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
          });
          db.close();
        `
      ],
      { stdio: ['ignore', 'pipe', 'inherit'] }
    );
    const exited = new Promise((resolve) => holder.on('exit', resolve));

    await new Promise((resolve, reject) => {
      holder.stdout.on('data', (data) => {
        if (data.toString().includes('holding')) {
          resolve();
        }
      });
      holder.on('exit', () => reject(new Error('the holder exited before holding the lock')));
    });

    let ticks = 0;
    const ticking = setInterval(() => {
      ticks += 1;
    }, 10);
    const started = Date.now();

    try {
      await db.writeAsync((txn) => txn.collection('users').insert({ name: 'Dave' }));
    } finally {
      clearInterval(ticking);
    }

    const waited = Date.now() - started;

    assert.equal(await exited, 0);
    assert.ok(waited >= 100, `the write waited for the other writer (${waited} ms)`);
    assert.ok(ticks >= 5, `timers ran while the write waited (${ticks} in ${waited} ms)`);
  });
});

describe('asynchronous opening', () => {
  const v2 = schema(2, {
    people: collection({
      fullName: t.string(),
      email: t.string().optional().unique(),
      age: t.string().default('')
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

  it('creates a database, and fails with the engine code', async (context) => {
    const path = tempPath(context);

    await assertRejects(Database.openAsync(path, { create: false }), 'NOT_FOUND');
    await assertRejects(Database.openAsync(path, { pageSize: 1000 }), 'INVALID_ARGUMENT');

    const db = await Database.openAsync(path);

    context.after(() => db.close());
    assert.equal(db.pageSize, 4096);
    assert.equal(db.schemaVersion, null);
  });

  it('runs asynchronous migration functions with the objects as they were', async (context) => {
    const { path, db } = await withData(context);
    const seen = [];
    // A handle without a schema, which a migration does not concern.
    const plain = Database.open(path);

    context.after(() => plain.close());
    await db.closeAsync();

    const migrated = await Database.openAsync(path, {
      schema: v2,
      migrations: steps(async (m) => {
        seen.push([m.previousVersion, m.version]);

        const people = m.collection('people');

        for (const key of await m.previousKeys('users')) {
          const before = await m.previous('users', key);

          // Not awaited: the step waits for its operations.
          people.put({ ...(await people.get(key)), age: `${before.age} years` });
        }

        // The migration holds the writer lock, so a write from its function
        // would wait for it.
        await assertRejects(
          plain.writeAsync(() => {}),
          'INVALID_ARGUMENT'
        );
        assert.throws(
          () => plain.write(() => {}),
          (error) => error.code === 'INVALID_ARGUMENT'
        );
      })
    });

    context.after(() => migrated.close());
    assert.deepEqual(seen, [[1, 2]]);
    assert.equal(migrated.schemaVersion, 2);
    assert.deepEqual(
      await migrated.readAsync(async (txn) =>
        (await txn.collection('people').find()).map((person) => [person.fullName, person.age])
      ),
      [
        ['Alice', '31 years'],
        ['Bob', '17 years'],
        ['Carol', '40 years']
      ]
    );
  });

  it('leaves the file as it was when a migration function rejects', async (context) => {
    const { path, db } = await withData(context);

    await db.closeAsync();
    await assert.rejects(
      Database.openAsync(path, {
        schema: v2,
        migrations: steps(async (m) => {
          await m.collection('people').delete(1);
          throw new Error('not today');
        })
      }),
      /not today/
    );

    const again = await Database.openAsync(path, { schema: v1 });

    context.after(() => again.close());
    assert.equal(await again.readAsync((txn) => txn.collection('users').count()), 3);
  });
});
