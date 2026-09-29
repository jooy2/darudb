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
