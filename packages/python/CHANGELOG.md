# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- The `upgrade_format` parameter of `Database.open` and `open_async`, and `upgrade_format` and `upgrade_format_async`, which say whether opening raises a file of an older format version, and raise an open one.

### Changed

- An index whose entries for one value come in key order, as a non-unique index's do while the objects' primary keys grow, fills its pages as it goes, where splitting a page in the middle left half of each empty, and so do the branches of every tree. A file of 400,000 sample objects with 14 indexes is 7% smaller for it. Keys in no particular order split pages evenly, as before.
- `compact` and `compact_async` first write again, full, every tree whose pages inserts left part empty, so that the file ends about as small as a backup's copy: the same file compacts to 119 MiB rather than 140 MiB.
- The file format is version 6, `FORMAT_VERSION`, which writes the lengths in a leaf's entries as varints rather than in five bytes. A file of 400,000 sample objects with 14 indexes takes 8% fewer pages, 126 MiB against 138, and compacts to 110 MiB against 119, with reads and writes as fast as before. Opening a file of version 5 raises it to version 6, while no other process has it open, by rewriting its header; its leaves take the new layout as writes change them, and `compact` rewrites the trees where that saves room. A release before this one cannot open a file of version 6, so an application that may go back to one opens with `upgrade_format=False`, which also creates new files in version 5, and calls `upgrade_format` once it no longer may.
- The native module is built with its loops aligned to 64 bytes, so that where a hot loop lands no longer depends on the rest of the build: in a development build, the position of one loop alone made one-object deferred commits 15% slower. It is about 7% larger for it.

### Fixed

- `compact` and `compact_async` no longer grow the file when a value in an overflow run near the file's end finds no run of free pages below to move into. The run stays where it is, and a round of moves that would grow the file is dropped.

## v1.0.0 (2026-10-07)

### Added

- `Database.open` opens or creates a database file, with a `Schema` of classes decorated with `@collection`, and the options `create`, `page_size`, `busy_timeout`, `cache_size`, `key`, `password` and `password_hashing`; a `key` and a `password` together are refused with `INVALID_ARGUMENT`, here and in `salvage` and `backup`. A file holding an older schema version is migrated in one write transaction, through the `Migration`s given for the versions in between, whose `run` functions read the old objects through `Migrating.previous`.
- `@collection` and `@embedded` make a class a frozen, keyword-only dataclass whose annotations are its fields' types: `bool`, `int`, `float`, `str`, `bytes`, lists of those, embedded classes, and any of them `| None` for an optional field. `field` adds a default, an index, a unique index, a primary key, a link to another collection, or a stored name of its own. A collection without a primary key is numbered by the engine, through its field `id: int | None = None`. An object read is built without the class's `__init__`, so a class with `__slots__` is refused.
- `db.read()` and `db.write()` are context managers: a write commits when its block ends, deferred with `durability="deferred"`, and aborts when it raises. `txn.collection(User)` gives a collection's objects as instances of the class: `get`, `find`, `find_one` and `count`, and in a write `insert`, `insert_many`, `put`, `put_many`, `update` and `delete`. Write transactions on one file do not nest: a second one in the same thread is refused with `INVALID_ARGUMENT`, and so are `sync`, `close`, `compact` and the key changes inside one, which would wait for it.
- Queries built from `F`: `F.age >= 18`, `F.address.city == "Seoul"` through embedded objects and links, `between`, `is_in`, `contains`, `startswith`, `endswith` and null tests, combined with `&`, `|` and `~`, in a `Query` with `where`, `sort_by`, `offset` and `limit`. Text in the query language with `$0`, `$1` and on, and `Database.prepare` with `param(0)` in a built query, compile a query once and run it with parameters.
- `Database.sync`, `close`, `set_key`, `set_password`, `is_encrypted`, `page_size`, `format_version` and `schema_version`. Every failure is a `DaruError` with the engine's error code.
- The asynchronous API for `asyncio`: `Database.open_async`, `read_async`, `write_async`, `sync_async`, `close_async` and an `_async` twin of every tool and key change, which run the engine's work on a thread of the package's pool. The operations of one transaction run in the order they were called, and an event loop's asynchronous writes on one file take turns. A synchronous write, sync or close on the file from the event loop's thread while one runs, and an asynchronous one awaited inside it or in a task made inside it, are refused with `INVALID_ARGUMENT`. A migration's `run` may be a coroutine function with `open_async`.
- The tools: `check`, the integrity check, which reports every problem in a `CheckReport`; `backup`, a copy of the published commit in a new file, under a new data key when it is given a `key` or a `password`; `compact`, which makes the file smaller in place; and `Database.salvage`, which rescues what it can of a damaged file into a new one.
- The native module releases the GIL while the engine works, and declares itself safe for free-threaded Python.
