# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- The `upgradeFormat` parameter of `Database.open` and `openAsync`, and `upgradeFormat` and `upgradeFormatAsync`, which say whether opening raises a file of an older format version, and raise an open one.

### Changed

- An index whose entries for one value come in key order, as a non-unique index's do while the objects' primary keys grow, fills its pages as it goes, where splitting a page in the middle left half of each empty, and so do the branches of every tree. A file of 400,000 sample objects with 14 indexes is 7% smaller for it. Keys in no particular order split pages evenly, as before.
- `compact` and `compactAsync` first write again, full, every tree whose pages inserts left part empty, so that the file ends about as small as a backup's copy: the same file compacts to 119 MiB rather than 140 MiB.
- The file format is version 6, the top-level `formatVersion`, which writes the lengths in a leaf's entries as varints rather than in five bytes. A file of 400,000 sample objects with 14 indexes takes 8% fewer pages, 126 MiB against 138, and compacts to 110 MiB against 119, with reads and writes as fast as before. Opening a file of version 5 raises it to version 6, while no other process has it open, by rewriting its header; its leaves take the new layout as writes change them, and `compact` rewrites the trees where that saves room. A release before this one cannot open a file of version 6, so an app that may go back to one opens with `upgradeFormat: false`, which also creates new files in version 5, and calls `upgradeFormat` once it no longer may.

### Fixed

- `compact` and `compactAsync` no longer grow the file when a value in an overflow run near the file's end finds no run of free pages below to move into. The run stays where it is, and a round of moves that would grow the file is dropped.
- The package resolves in a Flutter app on Flutter 3.38 to 3.44, the oldest releases it supports. It asked for versions of `hooks`, `code_assets` and `native_toolchain_rust` that need `meta` 1.19, which those Flutter releases pin lower, so `flutter pub get` failed there.

## v1.0.1 (2026-10-07)

### Added

- An example, `example/main.dart`, which stores a few objects, finds them by an index in code and as text, changes one, and opens the file again. pub.dev shows it on the package's Example tab.

## v1.0.0 (2026-10-05)

### Added

- `Database.open` opens or creates a database file, with a `Schema` of the collections `darudb_generator` generates from classes annotated `@Collection()` and `@Embedded()`, and the options `create`, `pageSize`, `busyTimeout`, `cacheSize`, `key`, `password` and `passwordHashing`; a `key` and a `password` together are refused with `INVALID_ARGUMENT`, here and in `salvage` and `backup`. A file holding an older schema version is migrated in one write transaction, through the `Migration`s registered for the versions in between, whose functions read the old objects through `MigrationContext`.
- `Database.read` and `Database.write` run a function in a transaction; `write` commits when it returns, deferred with `Durability.deferred`, and aborts when it throws. `txn.collection(userSchema)` gives a collection's objects as the annotated class: `get`, `find`, `findOne` and `count` with a typed query builder whose conditions combine with `&`, `|` and `~`, `findText` for the query language, `Database.prepare` and `findPrepared`, and in a write `insert`, `insertMany`, `put`, `putMany`, `update` and `delete`. `update` takes a change for each field it sets, made by the field's `set`, an embedded object's included, which it replaces whole.
- `Database.sync`, `Database.close`, `setKey`, `setPassword`, `isEncrypted`, `pageSize`, `formatVersion` and `schemaVersion`. Every failure is a `DaruException` with the engine's error code.
- The `Future` API: `Database.openAsync`, `readAsync`, `writeAsync`, `syncAsync`, `closeAsync`, `setKeyAsync` and `setPasswordAsync`, whose transactions give collections of `Future`-returning calls. The engine's work runs on threads of the native library, and the result comes back on the isolate's event loop. The calls of one transaction run in the order they were made; an isolate's asynchronous writes on one file take turns, and a synchronous `write`, `sync` or `close` on the file while one runs is refused with `INVALID_ARGUMENT`.
- The tools, each with an `Async` twin: `check`, the integrity check, which reports every problem in a `CheckReport`; `backup`, a copy of the published commit in a new file, under a new data key when it is given a `key` or a `password` for the copy; `compact`, which makes the file smaller in place; and `Database.salvage`, which rescues what it can of a damaged file into a new one, with a key or a password for an encrypted file.
- The build hook downloads the native library for the application's target from the package's GitHub release, and uses it only when its SHA-256 hash is the one `hook/prebuilt.json` names, so building an application needs no Rust toolchain. A download is kept in the hook's cache. In a checkout of the repository, which an application gets by depending on the package through git or a path, the hook builds the engine from source with `native_toolchain_rust` instead.
