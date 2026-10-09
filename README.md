<img src="docs/public/logo-256.png" alt="" width="128" height="128">

# DaruDB

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE) [![run-test-rust](https://github.com/jooy2/darudb/actions/workflows/run-test-rust.yml/badge.svg)](https://github.com/jooy2/darudb/actions/workflows/run-test-rust.yml) [![run-test-node](https://github.com/jooy2/darudb/actions/workflows/run-test-node.yml/badge.svg)](https://github.com/jooy2/darudb/actions/workflows/run-test-node.yml) [![run-test-python](https://github.com/jooy2/darudb/actions/workflows/run-test-python.yml/badge.svg)](https://github.com/jooy2/darudb/actions/workflows/run-test-python.yml)

### [**darudb.cdget.com**](https://darudb.cdget.com)

Guides and the full API. This README covers the essentials.

---

**An embedded database that keeps an application's data in one local file, for Rust, Node.js, Dart and Python.**

DaruDB is a database that runs inside your application rather than beside it. There is no server to start and nothing to configure: a program opens a file, reads and writes objects in it, and closes it. The engine is written once, in Rust, and each language reaches the same engine through a thin binding, so a file written from Node.js reads the same from Rust, Dart or Python.

## Why DaruDB

These are the goals the design is built around, in order of priority. They are goals rather than claims until the benchmarks and the test suites that prove each one are in this repository.

- **Fast reads and writes.** The target is to outperform the embedded SQL engines applications usually reach for, measured at the same durability settings. Records cross the language boundary as packed binary buffers, and batch operations are the default.
- **Encryption of the whole file.** Every page is encrypted and authenticated, so a file read without its key shows nothing, and a file changed without its key is detected. A password is turned into a key with a memory-hard function, and changing it does not rewrite the file.
- **A file that survives a crash.** A committed page is never overwritten in place, and a commit becomes visible by flipping a single byte. A process killed at any moment, or a power cut, leaves the last committed state intact. Every page carries a checksum, and the tools to check, repair, back up and compact a file ship with the library.
- **Several processes on one file.** Only operating-system file locks coordinate processes, never shared memory, so a process that dies holding a lock cannot leave the others stuck. One process writes at a time while any number read.
- **Runs where your application runs.** Windows, macOS, Linux, iOS and Android, including older releases of each. The file format carries its version, and each new version ships with the migration from the one before it, as does each version of your schema.
- **Easy to use from each language.** Declare a schema once and query with a type-safe builder in each language: filters, sorting, links between objects, and indexes. Results are plain objects, not handles that break when the database closes.

## Packages

| Package                                                            | Registry                                                                 | Requires                    | Status   |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------ | --------------------------- | -------- |
| [`crates/darudb`](crates/darudb)                                   | [crates.io: `darudb`](https://crates.io/crates/darudb)                   | Rust 1.85 or later          | Released |
| [`packages/node`](packages/node)                                   | [npm: `darudb`](https://www.npmjs.com/package/darudb)                    | Node.js 20 or later         | Released |
| [`packages/dart/darudb`](packages/dart/darudb)                     | [pub.dev: `darudb`](https://pub.dev/packages/darudb)                     | Dart 3.10 or Flutter 3.38.1 | Released |
| [`packages/dart/darudb_generator`](packages/dart/darudb_generator) | [pub.dev: `darudb_generator`](https://pub.dev/packages/darudb_generator) | Dart 3.10 or Flutter 3.38.1 | Released |
| [`packages/python`](packages/python)                               | [PyPI: `darudb`](https://pypi.org/project/darudb/)                       | CPython 3.11 or later       | Released |

The Rust crate is the engine itself. The other packages bind it to their language and add nothing to what it does, so every language reads and writes the same file in the same way. Each package **versions independently and keeps its own changelog** beside its own manifest.

## Repository layout

| Path              | What it is                                      | How it is run                                                          |
| ----------------- | ----------------------------------------------- | ---------------------------------------------------------------------- |
| `crates/darudb`   | The engine and the Rust API                     | `cargo test -p darudb` from the repository root                        |
| `packages/node`   | The Node.js binding                             | `cd packages/node && npm install`, then `npm run build` and `npm test` |
| `packages/python` | The Python binding                              | `cd packages/python`, then `maturin develop` and `pytest` in a venv    |
| `docs`            | The documentation site, shared by every package | `cd docs && npm install`, then `npm run dev`                           |
| `design`          | The engine's specifications                     | Read, in English                                                       |
| `samples`         | Sample apps with end-to-end tests, not released | [samples/README.md](samples/README.md)                                 |

The root holds the Cargo workspace and no JavaScript or Python manifest. Each JavaScript and Python folder is entered and run on its own. [CONTRIBUTING.md](CONTRIBUTING.md) has the rest.

## Documentation

| Page                                                                  | What you will find                                       |
| --------------------------------------------------------------------- | -------------------------------------------------------- |
| [**Introduction**](https://darudb.cdget.com/guide/introduction)       | What DaruDB is, what it is for, and how far along it is. |
| [**Getting started**](https://darudb.cdget.com/guide/getting-started) | Installing a package and opening a first database.       |
| [**Collections and objects**](https://darudb.cdget.com/guide/objects) | Schemas, objects, queries and migrations, in Rust.       |
| [**Node.js**](https://darudb.cdget.com/guide/nodejs)                  | The same in JavaScript and TypeScript.                   |
| [**Changelog**](https://darudb.cdget.com/changelog)                   | What changed in each package.                            |

## Contributing

Bug reports, feature requests and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) says how, and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) is the conduct this project holds itself to. For anything with a security impact, do **not** open an issue; [SECURITY.md](SECURITY.md) has the private route.

## License

[MIT](LICENSE) © [CDGet](https://cdget.com)
