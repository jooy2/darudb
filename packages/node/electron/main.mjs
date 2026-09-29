/**
 * The package in Electron's main process, where desktop applications open
 * their databases. Electron embeds its own Node.js and V8, with rules of its
 * own: V8's memory cage there refuses buffers whose memory lives outside
 * it, which is how a native addon most easily hands bytes to JavaScript. So
 * this runs what crosses the boundary in each direction, synchronously and on
 * the thread pool, an encrypted file, and the tools, and exits with 0 when
 * all of it works and 1 otherwise.
 *
 * Run with `npm test` in this folder, after `npm run build` in the package.
 */
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { app } from 'electron';

import { collection, Database, schema, t } from '../dist/index.js';

const app1 = schema(1, {
  files: collection({
    name: t.string().unique(),
    size: t.int().index(),
    bytes: t.bytes().optional()
  })
});

/** A file of `n` bytes, with bytes large enough to leave the reused buffer. */
const file = (n) => ({ name: `file ${n}`, size: n, bytes: new Uint8Array(n % 7 === 0 ? 5000 : 3) });

async function run(dir) {
  const path = join(dir, 'app.darudb');
  const db = Database.open(path, { schema: app1 });

  db.write((txn) =>
    txn.collection('files').insertMany(Array.from({ length: 500 }, (_, n) => file(n)))
  );

  const large = db.read((txn) => txn.collection('files').find((q) => q.where('size', '>=', 100)));

  assert.equal(large.length, 400);
  assert.ok(large.every((found) => found.bytes instanceof Uint8Array));
  assert.equal(db.read((txn) => txn.collection('files').get(8)).name, 'file 7');
  assert.equal(db.check().ok, true);
  db.close();

  const later = await Database.openAsync(path, { schema: app1 });

  await later.writeAsync(async (txn) => {
    await txn.collection('files').put({ id: 1, name: 'renamed', size: 0 });
  });
  assert.equal(
    await later.readAsync((txn) => txn.collection('files').count((q) => q.where('size', '==', 0))),
    1
  );
  assert.equal((await later.backupAsync(join(dir, 'copy.darudb'))).entries > 500, true);
  await later.closeAsync();

  const secret = Database.open(join(dir, 'secret.darudb'), {
    schema: app1,
    password: 'correct horse',
    passwordHashing: { memoryKib: 1024, iterations: 1, parallelism: 1 }
  });

  assert.equal(secret.isEncrypted, true);
  secret.close();
}

app.whenReady().then(async () => {
  const dir = mkdtempSync(join(tmpdir(), 'darudb-electron-'));
  let code = 0;

  try {
    await run(dir);
    console.log(`darudb works in Electron ${process.versions.electron}`);
  } catch (error) {
    console.error(error);
    code = 1;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }

  app.exit(code);
});
