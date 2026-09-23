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

import { Database, engineVersion, FORMAT_VERSION } from '../index.js';

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

    for (const pageSize of [0, 256, 1000, 131072]) {
      assertCode(
        () => Database.open(join(dir, `app-${pageSize}.darudb`), { pageSize }),
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
