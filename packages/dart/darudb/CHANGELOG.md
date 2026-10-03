# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- `Database.open` opens or creates a database file, with a `Schema` of the collections `darudb_generator` generates from classes annotated `@Collection()` and `@Embedded()`, and the options `create`, `pageSize`, `busyTimeout`, `cacheSize`, `key`, `password` and `passwordHashing`. A file holding an older schema version is migrated in one write transaction, through the `Migration`s registered for the versions in between, whose functions read the old objects through `MigrationContext`.
- `Database.read` and `Database.write` run a function in a transaction; `write` commits when it returns, deferred with `Durability.deferred`, and aborts when it throws. `txn.collection(userSchema)` gives a collection's objects as the annotated class: `get`, `find`, `findOne` and `count` with a typed query builder whose conditions combine with `&`, `|` and `~`, `findText` for the query language, `Database.prepare` and `findPrepared`, and in a write `insert`, `insertMany`, `put`, `putMany`, `update` and `delete`.
- `Database.sync`, `Database.close`, `setKey`, `setPassword`, `isEncrypted`, `pageSize`, `formatVersion` and `schemaVersion`. Every failure is a `DaruException` with the engine's error code.
- The build hook builds the engine from source with `native_toolchain_rust`.
