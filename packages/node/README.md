# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The Node.js package of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file. It is a native addon over the DaruDB engine, which is written in Rust, so the same file reads the same from Node.js, Rust and Dart.

> DaruDB is in early development. The package is not published yet, and the file format will change without a migration path until the first release.

## Usage

A schema declares the collections, and TypeScript infers every object's type from it. Transactions run a function: `write` commits when it returns and aborts when it throws, and `read` sees one commit throughout.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index()
  })
});

const db = Database.open('app.darudb', { schema: app });

db.write((txn) => {
  txn.collection('users').insertMany([
    { name: 'Alice', age: 31 },
    { name: 'Bob', email: 'bob@example.com' }
  ]);
});

const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);
const same = db.read((txn) =>
  txn.collection('users').find('age >= $0 SORT BY age DESC LIMIT 10', [18])
);

db.close();
```

Objects and queries cross into the engine as bytes, one buffer per call, so a batch of objects costs one call. Raising the schema's version migrates the file when it opens, with renames, deletions and a JavaScript function of your own; [the guide](https://darudb.cdget.com/guide/nodejs) has the details.

Every error thrown by the package is an `Error` with a stable `code`, the same one in every language DaruDB ships to:

```js
try {
  Database.open('missing.darudb', { create: false });
} catch (error) {
  if (error.code === 'NOT_FOUND') {
    // Nothing exists at that path.
  }
}
```

## Requirements

Node.js 20 or later. The package ships prebuilt binaries for macOS, Windows, Linux (glibc and musl), FreeBSD and Android, so installing it compiles nothing.

## Building from source

The addon is built from the Rust source in this folder and the engine in `crates/darudb`. You need Rust (the version in the repository's `rust-toolchain.toml` is installed automatically by `rustup`) and Node.js.

```bash
npm install
npm run build
```

`npm run build` writes the addon for your platform and the files that load it. `npm test` runs the tests against it and checks the TypeScript declarations.

## License

[MIT](https://github.com/jooy2/darudb/blob/main/LICENSE) © [CDGet](https://cdget.com)
