# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- `Database.open` opens a database file, creating it when nothing exists at the path, with `create` and `pageSize` options. `path`, `isOpen`, `pageSize` and `formatVersion` describe the open database, and `close` flushes and closes it.
- Every error thrown by the package carries the engine's stable `code`, such as `NOT_FOUND`, `NOT_A_DATABASE` or `CLOSED`.
- Several processes can open one database file at once, coordinated through the engine's file locks. A database on a network file system is refused with `UNSUPPORTED_FILE_SYSTEM`.
- `engineVersion()` and `FORMAT_VERSION` report the engine inside the package and the file format version it reads and writes.
- Collections of objects under a schema declared with `t`, `collection` and `schema`, passed to `Database.open` as `schema`. TypeScript infers every object's type from the declaration. `db.read(fn)` and `db.write(fn)` run a function in a transaction, committing a write when the function returns and aborting it when it throws, with `durability: 'deferred'` to commit without waiting for the disk. A collection offers `get`, `find`, `findOne`, `count`, `insert`, `insertMany`, `put`, `putMany` and `delete`, and objects cross into the engine as records, a batch in one call. A property the schema does not have is refused, `t.bigint()` declares an int read as a `bigint`, write transactions on one file do not nest, even through two `Database` objects or from a migration function, and `busyTimeout` sets how long a write waits for another process.
- Queries built with `where`, `sortBy`, `limit` and `offset`, typed by the collection's fields, or written in the query language with parameters. `conditions` and `Query` build filters outside a collection.
- Migrations: raising the schema's version migrates the file when it opens, with the renames, deletions and replaced fields a migration lists and a JavaScript function that reads the objects as they were through `previous` and `previousKeys`.
- `db.schemaVersion`, and `db.sync()` to make deferred commits durable.
- An asynchronous API: `Database.openAsync`, `readAsync`, `writeAsync`, `syncAsync` and `closeAsync` do the engine's work on the libuv thread pool and resolve promises, so the event loop never waits for the disk or for another process's writer. Their transaction functions and migration functions may be asynchronous, every collection method returns a promise, and a transaction runs its operations in the order they were called and commits once they have all settled. This process's writes on one file queue in JavaScript rather than on the pool, `syncAsync` and `closeAsync` with them, and a synchronous `write`, `sync` or `close` while an asynchronous write holds the file fails with `INVALID_ARGUMENT` rather than blocking the event loop.
