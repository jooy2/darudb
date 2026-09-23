# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The Node.js package of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file. It is a native addon over the DaruDB engine, which is written in Rust, so the same file reads the same from Node.js, Rust and Dart.

> DaruDB is in early development. The package is not published yet, it cannot store data yet, and the file format will change without a migration path until the first release.

## Usage

```js
import { Database } from 'darudb';

// Opens the database, creating the file if it does not exist.
const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();
```

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

`npm run build` writes the addon for your platform, `index.js` and `index.d.ts`.

## License

[MIT](https://github.com/jooy2/darudb/blob/main/LICENSE) © [CDGet](https://cdget.com)
