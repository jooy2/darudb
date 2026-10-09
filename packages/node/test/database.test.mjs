/**
 * The Node.js binding, through the package's public entry point.
 *
 * These tests run against the addon `npm run build` left in the package
 * folder, so build first after any change to Rust code. What the engine
 * decides is tested in Rust; what is checked here is that it arrives in
 * JavaScript intact — values as the right types, errors with the engine's
 * `code`, and the handle's life after `close`.
 */
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { collection, Database, engineVersion, FORMAT_VERSION, schema, t } from '../dist/index.js';

/**
 * A directory of the test's own, removed when the test ends, so tests never
 * see each other's files.
 */
const tempDir = (context) => {
  const dir = mkdtempSync(join(tmpdir(), 'darudb-node-'));

  context.after(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  return dir;
};

/** Asserts that `run` throws an `Error` whose `code` is `code`. */
const assertCode = (run, code) => {
  assert.throws(run, (error) => {
    assert.ok(error instanceof Error, 'a real Error is thrown');
    assert.equal(error.code, code);
    assert.ok(error.message.length > 0, 'the error has a message');

    return true;
  });
};

describe('the module', () => {
  it('reports the engine version and the file format version', () => {
    assert.match(engineVersion(), /^\d+\.\d+\.\d+/);
    assert.equal(typeof FORMAT_VERSION, 'number');
  });

  it('has no constructor for a database', () => {
    assert.throws(() => new Database());
  });
});

describe('Database.open', () => {
  it('creates a database where nothing exists', (context) => {
    const path = join(tempDir(context), 'app.darudb');
    const db = Database.open(path);

    assert.equal(db.path, path);
    assert.equal(db.isOpen, true);
    assert.equal(db.pageSize, 4096);
    assert.equal(db.formatVersion, FORMAT_VERSION);
    db.close();

    assert.equal(readFileSync(path).length, 4096);
  });

  it('opens a database again with the page size it recorded', (context) => {
    const path = join(tempDir(context), 'app.darudb');

    Database.open(path, { pageSize: 16384 }).close();

    const db = Database.open(path, { create: false });

    assert.equal(db.pageSize, 16384);
    db.close();
  });

  it('refuses to create a database when `create` is false', (context) => {
    const path = join(tempDir(context), 'app.darudb');

    assertCode(() => Database.open(path, { create: false }), 'NOT_FOUND');
  });

  it('refuses a page size that is not a power of two in range', (context) => {
    const dir = tempDir(context);

    for (const pageSize of [0, 512, 2048, 1000, 131072]) {
      assertCode(
        () => Database.open(join(dir, `app-${pageSize}.darudb`), { pageSize }),
        'INVALID_ARGUMENT'
      );
    }
  });

  it('takes a cache size in bytes, and refuses one that is not a whole number', (context) => {
    const dir = tempDir(context);

    for (const cacheSize of [0, 1 << 20, 2 ** 40]) {
      const db = Database.open(join(dir, `app-${cacheSize}.darudb`), { cacheSize });

      assert.equal(db.isOpen, true);
      db.close();
    }

    for (const cacheSize of [-1, 1.5, '1024', Number.NaN, Infinity]) {
      assertCode(
        () => Database.open(join(dir, 'refused.darudb'), { cacheSize }),
        'INVALID_ARGUMENT'
      );
    }
  });

  it('refuses a file that is not a database', (context) => {
    const path = join(tempDir(context), 'notes.txt');

    writeFileSync(path, 'a shopping list, not a database');

    assertCode(() => Database.open(path), 'NOT_A_DATABASE');
  });
});

describe('Database#close', () => {
  it('leaves only `path` and `isOpen` readable afterwards', (context) => {
    const path = join(tempDir(context), 'app.darudb');
    const db = Database.open(path);

    db.close();

    assert.equal(db.isOpen, false);
    assert.equal(db.path, path);
    assertCode(() => db.pageSize, 'CLOSED');
    assertCode(() => db.formatVersion, 'CLOSED');
  });

  it('does nothing the second time', (context) => {
    const db = Database.open(join(tempDir(context), 'app.darudb'));

    db.close();

    assert.doesNotThrow(() => db.close());
  });
});

describe('Database#check', () => {
  const people = schema(1, {
    people: collection({ name: t.string(), email: t.string().unique(), age: t.int().index() })
  });

  it('finds nothing wrong with a whole file, synchronously and on the thread pool', async (context) => {
    const db = Database.open(join(tempDir(context), 'app.darudb'), { schema: people });

    db.write((txn) =>
      txn
        .collection('people')
        .insertMany(
          Array.from({ length: 200 }, (_, n) => ({ name: `p${n}`, email: `${n}@x`, age: n % 9 }))
        )
    );

    const report = db.check();

    assert.equal(report.ok, true);
    assert.deepEqual(report.problems, []);
    assert.equal(report.objectsChecked, 200);
    assert.ok(report.pagesChecked > 3);
    assert.deepEqual(await db.checkAsync(), report);
    db.close();
    assertCode(() => db.check(), 'CLOSED');
  });

  it('names a damaged page, and reports rather than throws', (context) => {
    const dir = tempDir(context);
    const path = join(dir, 'app.darudb');
    const db = Database.open(path, { schema: people });

    db.write((txn) =>
      txn
        .collection('people')
        .insertMany(
          Array.from({ length: 200 }, (_, n) => ({ name: `p${n}`, email: `${n}@x`, age: n % 9 }))
        )
    );

    const pageSize = db.pageSize;

    db.close();

    const image = readFileSync(path);
    let named = 0;

    for (let page = 1; page < image.length / pageSize; page++) {
      const damaged = Buffer.from(image);
      const copy = join(dir, `damaged-${page}.darudb`);

      damaged[page * pageSize + 2000] ^= 0x55;
      writeFileSync(copy, damaged);

      let opened;

      try {
        opened = Database.open(copy, { schema: people });
      } catch (error) {
        // Damage to what opening reads stops it opening.
        assert.equal(error.code, 'CORRUPTED');

        continue;
      }

      const report = opened.check();

      opened.close();

      if (report.problems.some((problem) => problem.page === page)) {
        assert.equal(report.ok, false);
        named++;
      }
    }

    assert.ok(named > 2, `${named} damaged pages named`);
  });
});

describe('encryption', () => {
  const people = schema(1, { people: collection({ name: t.string() }) });
  // A small hashing cost, so that the tests do not wait for Argon2id.
  const cheap = { memoryKib: 1024, iterations: 1, parallelism: 1 };
  const key = Uint8Array.from({ length: 32 }, (_, n) => n);
  const otherKey = Uint8Array.from({ length: 32 }, (_, n) => 255 - n);

  const filled = (path, options) => {
    const db = Database.open(path, { schema: people, ...options });

    db.write((txn) => txn.collection('people').insert({ name: 'Ada' }));

    return db;
  };
  const names = (db) =>
    db.read((txn) =>
      txn
        .collection('people')
        .find()
        .map((p) => p.name)
    );

  it("opens with its key only, and leaves the caller's buffer as it was", (context) => {
    const path = join(tempDir(context), 'keyed.darudb');
    const given = Uint8Array.from(key);

    filled(path, { key: given }).close();

    assert.deepEqual(given, key);

    const db = Database.open(path, { schema: people, key });

    assert.equal(db.isEncrypted, true);
    assert.deepEqual(names(db), ['Ada']);
    db.close();

    assertCode(() => Database.open(path, { schema: people }), 'KEY_REQUIRED');
    assertCode(() => Database.open(path, { schema: people, key: otherKey }), 'WRONG_KEY');
    assertCode(
      () => Database.open(path, { schema: people, password: 'a guess', passwordHashing: cheap }),
      'WRONG_KEY'
    );
  });

  it('refuses a key or a password it cannot use', (context) => {
    const dir = tempDir(context);
    const plain = Database.open(join(dir, 'plain.darudb'));

    assert.equal(plain.isEncrypted, false);
    assertCode(() => plain.setKey(key), 'INVALID_ARGUMENT');
    plain.close();

    assertCode(() => Database.open(join(dir, 'plain.darudb'), { key }), 'INVALID_ARGUMENT');
    assertCode(
      () => Database.open(join(dir, 'a.darudb'), { key: key.subarray(1) }),
      'INVALID_ARGUMENT'
    );
    assertCode(
      () => Database.open(join(dir, 'b.darudb'), { key: 'not bytes' }),
      'INVALID_ARGUMENT'
    );
    assertCode(
      () => Database.open(join(dir, 'c.darudb'), { key, password: 'both' }),
      'INVALID_ARGUMENT'
    );
    assertCode(() => Database.open(join(dir, 'd.darudb'), { password: '' }), 'INVALID_ARGUMENT');
    assertCode(
      () => Database.open(join(dir, 'f.darudb'), { password: 'p', passwordHashing: null }),
      'INVALID_ARGUMENT'
    );
    assertCode(
      () =>
        Database.open(join(dir, 'e.darudb'), {
          password: 'p',
          passwordHashing: { memoryKib: -1, iterations: 1, parallelism: 1 }
        }),
      'INVALID_ARGUMENT'
    );
  });

  it('changes the key and the password, synchronously and on the thread pool', async (context) => {
    const path = join(tempDir(context), 'changed.darudb');
    const db = filled(path, { password: 'first', passwordHashing: cheap });
    const reopen = (secret) => {
      const opened = Database.open(path, { schema: people, ...secret });
      const found = names(opened);

      opened.close();

      return found;
    };

    db.setPassword('second');
    await db.setPasswordAsync(new TextEncoder().encode('third'));
    db.close();

    assertCode(() => reopen({ password: 'second' }), 'WRONG_KEY');
    assert.deepEqual(reopen({ password: 'third' }), ['Ada']);

    const again = Database.open(path, { schema: people, password: 'third' });

    again.setKey(key);
    await again.setKeyAsync(otherKey);
    again.close();

    assertCode(() => reopen({ key }), 'WRONG_KEY');
    assert.deepEqual(reopen({ key: otherKey }), ['Ada']);
  });

  it('refuses a key change while an asynchronous write holds the file', async (context) => {
    const db = filled(join(tempDir(context), 'held.darudb'), { key });
    let refused;

    await db.writeAsync(async () => {
      try {
        db.setKey(otherKey);
      } catch (error) {
        refused = error.code;
      }
    });

    assert.equal(refused, 'INVALID_ARGUMENT');
    db.close();
  });

  it('opens on the thread pool, and backs up and salvages with the same key', async (context) => {
    const dir = tempDir(context);
    const path = join(dir, 'app.darudb');

    filled(path, { key }).close();

    const db = await Database.openAsync(path, { schema: people, key });

    await db.backupAsync(join(dir, 'copy.darudb'));
    await db.closeAsync();

    assertCode(() => Database.salvage(path, join(dir, 'none.darudb')), 'KEY_REQUIRED');

    const report = await Database.salvageAsync(path, join(dir, 'rescued.darudb'), { key });

    assert.equal(report.whole, true);

    for (const name of ['copy.darudb', 'rescued.darudb']) {
      assertCode(() => Database.open(join(dir, name), { schema: people }), 'KEY_REQUIRED');

      const opened = Database.open(join(dir, name), { schema: people, key });

      assert.deepEqual(names(opened), ['Ada']);
      opened.close();
    }
  });
});

describe('Database#backup', () => {
  const people = schema(1, {
    people: collection({ name: t.string(), email: t.string().unique(), age: t.int().index() })
  });

  it('copies the database to a new file, synchronously and on the thread pool', async (context) => {
    const dir = tempDir(context);
    const db = Database.open(join(dir, 'app.darudb'), { schema: people });

    db.write((txn) =>
      txn
        .collection('people')
        .insertMany(
          Array.from({ length: 300 }, (_, n) => ({ name: `p${n}`, email: `${n}@x`, age: n % 9 }))
        )
    );

    const report = db.backup(join(dir, 'copy.darudb'));
    const later = await db.backupAsync(join(dir, 'later.darudb'));

    for (const [name, made] of [
      ['copy.darudb', report],
      ['later.darudb', later]
    ]) {
      const copy = Database.open(join(dir, name), { schema: people });

      assert.equal(made.bytes, readFileSync(join(dir, name)).length);
      assert.ok(made.entries > 300, `${made.entries}`);
      assert.deepEqual(
        copy.read((txn) => txn.collection('people').find()),
        db.read((txn) => txn.collection('people').find())
      );
      assert.equal(copy.check().ok, true);
      copy.close();
    }

    assertCode(() => db.backup(join(dir, 'copy.darudb')), 'INVALID_ARGUMENT');
    await assert.rejects(db.backupAsync(join(dir, 'copy.darudb')), { code: 'INVALID_ARGUMENT' });
    assertCode(() => db.backup(''), 'INVALID_ARGUMENT');
    db.close();
  });

  it('encrypts the copy under a new data key with a key or a password', async (context) => {
    const dir = tempDir(context);
    const key = new Uint8Array(32).fill(3);
    const newKey = new Uint8Array(32).fill(5);
    const cheap = { memoryKib: 8192, iterations: 1, parallelism: 1 };
    const db = Database.open(join(dir, 'app.darudb'), { schema: people, key });
    const plain = Database.open(join(dir, 'plain.darudb'), { schema: people });

    for (const each of [db, plain]) {
      each.write((txn) => txn.collection('people').insert({ name: 'a', email: 'a@x', age: 1 }));
    }

    db.backup(join(dir, 'password.darudb'), { password: 'new', passwordHashing: cheap });
    await db.backupAsync(join(dir, 'key.darudb'), { key: newKey });
    plain.backup(join(dir, 'encrypted.darudb'), { key: newKey });

    for (const [name, secret] of [
      ['password.darudb', { password: 'new' }],
      ['key.darudb', { key: newKey }],
      ['encrypted.darudb', { key: newKey }]
    ]) {
      const copy = Database.open(join(dir, name), { schema: people, ...secret });

      assert.equal(copy.isEncrypted, true);
      assert.deepEqual(
        copy.read((txn) => txn.collection('people').find()),
        db.read((txn) => txn.collection('people').find())
      );
      copy.close();
    }

    assert.deepEqual(newKey, new Uint8Array(32).fill(5), 'the caller wipes its own key');
    assertCode(() => Database.open(join(dir, 'key.darudb'), { schema: people, key }), 'WRONG_KEY');
    assertCode(
      () => Database.open(join(dir, 'encrypted.darudb'), { schema: people }),
      'KEY_REQUIRED'
    );
    assertCode(
      () => db.backup(join(dir, 'both.darudb'), { key: newKey, password: 'x' }),
      'INVALID_ARGUMENT'
    );
    assertCode(() => db.backup(join(dir, 'empty.darudb'), { password: '' }), 'INVALID_ARGUMENT');
    assertCode(
      () =>
        db.backup(join(dir, 'cost.darudb'), { password: 'x', passwordHashing: { memoryKib: -1 } }),
      'INVALID_ARGUMENT'
    );
    db.close();
    plain.close();
  });
});

describe('Database.salvage', () => {
  const people = schema(1, {
    people: collection({ name: t.string(), email: t.string().unique(), age: t.int().index() })
  });

  /** A closed database of 300 people, written twice so older pages remain. */
  const filled = (path) => {
    const db = Database.open(path, { schema: people });

    for (const round of [0, 1]) {
      db.write((txn) => {
        const users = txn.collection('people');

        for (let n = 1; n <= 300; n++) {
          users.put({ id: n, name: `p${n} ${round}`, email: `${n}@x`, age: n % 9 });
        }
      });
    }

    const found = db.read((txn) => txn.collection('people').find());

    db.close();

    return found;
  };

  it('rescues a file into a new one, synchronously and on the thread pool', async (context) => {
    const dir = tempDir(context);
    const path = join(dir, 'app.darudb');
    const before = filled(path);
    const report = Database.salvage(path, join(dir, 'copy.darudb'));
    const later = await Database.salvageAsync(path, join(dir, 'later.darudb'), {
      busyTimeout: 100
    });

    for (const [name, made] of [
      ['copy.darudb', report],
      ['later.darudb', later]
    ]) {
      const copy = Database.open(join(dir, name), { schema: people });

      assert.equal(made.whole, true);
      assert.equal(typeof made.commitId, 'number');
      assert.equal(made.pagesDamaged, 0);
      assert.equal(made.bytes, readFileSync(join(dir, name)).length);
      assert.deepEqual(
        copy.read((txn) => txn.collection('people').find()),
        before
      );
      assert.equal(copy.check().ok, true);
      copy.close();
    }
  });

  it('rescues a damaged file and counts the damage', async (context) => {
    const dir = tempDir(context);
    const path = join(dir, 'app.darudb');

    filled(path);

    const bytes = readFileSync(path);

    bytes[4096 + 200] ^= 0xff;
    writeFileSync(path, bytes);

    const report = Database.salvage(path, join(dir, 'copy.darudb'));
    const copy = Database.open(join(dir, 'copy.darudb'), { schema: people });

    assert.equal(report.pagesDamaged, 1);
    assert.equal(copy.check().ok, true);
    copy.close();
  });

  it('refuses a file in use and a path that is taken', async (context) => {
    const dir = tempDir(context);
    const path = join(dir, 'app.darudb');

    filled(path);

    const db = Database.open(path, { schema: people });

    assertCode(() => Database.salvage(path, join(dir, 'copy.darudb')), 'BUSY');
    await assert.rejects(Database.salvageAsync(path, join(dir, 'copy.darudb')), {
      code: 'BUSY'
    });
    db.close();

    writeFileSync(join(dir, 'taken.darudb'), 'taken');
    assertCode(() => Database.salvage(path, join(dir, 'taken.darudb')), 'INVALID_ARGUMENT');
    assertCode(() => Database.salvage('', join(dir, 'copy.darudb')), 'INVALID_ARGUMENT');
    assertCode(
      () => Database.salvage(join(dir, 'none.darudb'), join(dir, 'copy.darudb')),
      'NOT_FOUND'
    );
  });
});

describe('format versions', () => {
  const notes = schema(1, { notes: collection({ text: t.string() }) });

  it('creates a file in format 5 when upgrading is off, and raises it by opening it again', (context) => {
    const path = join(tempDir(context), 'app.darudb');
    let db = Database.open(path, { upgradeFormat: false });

    assert.equal(FORMAT_VERSION, 6);
    assert.equal(db.formatVersion, 5);
    db.close();
    assert.equal(readFileSync(path)[8], 5);

    db = Database.open(path, { upgradeFormat: false });
    assert.equal(db.formatVersion, 5, 'not raised while upgrading is off');
    db.close();

    db = Database.open(path);
    assert.equal(db.formatVersion, 6);
    db.close();
    assert.equal(readFileSync(path)[8], 6);
  });

  it('raises an open file with the call, synchronously and on the thread pool', async (context) => {
    const dir = tempDir(context);

    for (const asynchronous of [false, true]) {
      const path = join(dir, `app-${asynchronous}.darudb`);
      const db = Database.open(path, { schema: notes, upgradeFormat: false });
      const id = db.write((txn) => txn.collection('notes').insert({ text: 'kept' }));
      const raised = asynchronous ? await db.upgradeFormatAsync() : db.upgradeFormat();

      assert.equal(raised, true);
      assert.equal(db.formatVersion, 6);
      assert.equal(db.upgradeFormat(), false, 'raised already');
      assert.deepEqual(
        db.read((txn) => txn.collection('notes').get(id)),
        { id, text: 'kept' }
      );
      assert.equal(db.check().ok, true);
      db.close();
    }
  });
});

describe('Database#compact', () => {
  const people = schema(1, {
    people: collection({ name: t.string(), email: t.string().unique(), age: t.int().index() })
  });

  it('makes a sparse file smaller and keeps every object, both ways', async (context) => {
    const dir = tempDir(context);

    for (const asynchronous of [false, true]) {
      const path = join(dir, `sparse-${asynchronous}.darudb`);
      const db = Database.open(path, { schema: people });

      db.write((txn) =>
        txn
          .collection('people')
          .insertMany(
            Array.from({ length: 3000 }, (_, n) => ({ name: `p${n}`, email: `${n}@x`, age: n % 9 }))
          )
      );
      db.write((txn) => {
        const users = txn.collection('people');

        for (let id = 1; id <= 3000; id++) {
          if (id % 10 !== 0) {
            users.delete(id);
          }
        }
      });

      const before = db.read((txn) => txn.collection('people').find());
      const report = asynchronous ? await db.compactAsync() : db.compact();

      assert.ok(report.pagesMoved > 0);
      assert.ok(report.bytesAfter < report.bytesBefore, JSON.stringify(report));
      assert.equal(report.bytesAfter, readFileSync(path).length);
      assert.deepEqual(
        db.read((txn) => txn.collection('people').find()),
        before
      );
      assert.equal(db.check().ok, true);
      db.close();
    }
  });

  it('is refused while an asynchronous write holds the file', async (context) => {
    const db = Database.open(join(tempDir(context), 'app.darudb'), { schema: people });
    let refused;

    await db.writeAsync(async () => {
      try {
        db.compact();
      } catch (error) {
        refused = error.code;
      }
    });

    assert.equal(refused, 'INVALID_ARGUMENT');
    db.close();
  });
});
