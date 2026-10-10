// The stores the Node.js harness runs, with the version of each, as JSON.
import Sqlite from 'better-sqlite3';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [, , packageDir] = process.argv;
const here = dirname(fileURLToPath(import.meta.url));
const versionOf = (name) =>
  JSON.parse(readFileSync(join(here, 'node_modules', name, 'package.json'), 'utf8')).version;
const daru = JSON.parse(readFileSync(join(packageDir, 'package.json'), 'utf8')).version;
const memory = new Sqlite(':memory:');
const sqlite = memory.prepare('SELECT sqlite_version() AS v').get().v;

memory.close();
process.stdout.write(
  JSON.stringify([
    { id: 'daru', version: daru },
    { id: 'sqlite', version: `${sqlite} (better-sqlite3 ${versionOf('better-sqlite3')})` },
    { id: 'lmdb', version: `lmdb-js ${versionOf('lmdb')}` }
  ]) + '\n'
);
