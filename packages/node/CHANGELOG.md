# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- `t.date()`, a field of instants, written and read as a `Date`: the file keeps a date to the microsecond, and a `Date` holds milliseconds, so a date read is rounded down to the millisecond. A date can be a list's element, a default, an indexed field and a sort's, a query compares it with a `Date`, in a condition, a parameter or `DATE "2026-10-30T12:00:00Z"` in the query language, and `min` and `max` give one. An invalid `Date` is refused with `INVALID_ARGUMENT`. The first date a schema declares moves the file to object format 2, which releases of the package before this one do not open; `upgradeFormat: false` refuses that with `INVALID_ARGUMENT`.
- `conditions.linked(collection, link, condition?)` holds for an object that some object of `collection` links to through its link field `link`, and that meets `condition`, as `ANY users.team (age >= 18)` does in the query language: `teams.find((q) => q.where((c) => c.linked('users', 'team', c.ge('age', 18))))`. `c.not` of it finds the objects nothing links to.
- Tests that ignore case: `where(field, op, value, { ignoreCase: true })`, and `conditions.ignoreCase(condition)` for every test of `eq`, `ne`, `in`, `contains`, `startsWith` and `endsWith` inside a condition, compare strings by Unicode's simple case folding, as `IGNORE CASE` does in the query language: `users.find('name == $0 IGNORE CASE', ['kim'])`. A test that orders, or one of a field holding no strings, fails with `INVALID_QUERY`. Each platform package's `THIRD_PARTY_NOTICES.txt` carries the licence notice of the Unicode data the folding comes from.
- `deleteWhere` and `updateWhere` on a write collection, and on an asynchronous one, delete or update every object a query finds, after its sort, offset and limit, and return how many there were: `users.updateWhere({ active: false }, (q) => q.where('age', '<', 18))`. The query is required, in any form `find` takes; `(q) => q` finds every object. An update by query is all or nothing: when one object is refused, by a unique value taken or an index entry too long, the objects changed before it are put back. It cannot set the primary key.
- Aggregates: `sum`, `min`, `max` and `avg` on a collection, and on an asynchronous one, work out one value from a field of the objects a query finds, after its sort, offset and limit, with the query in any form `find` takes: `users.sum('age', (q) => q.where('age', '>', 18))`, `users.max('age', 'team == $0', ['north'])`. The field may be a path through embedded objects and links. Nulls are left out; a sum of no values is 0 and the rest are `null`. A sum is exact, and a `t.bigint()` field sums to a `bigint`; a mean is a number. TypeScript allows `sum` and `avg` only on number fields, and types the result by the field.

## v1.4.0 (2026-10-29)

### Added

- `plain` in the options of `backup` and `backupAsync`, which writes the copy with no encryption: the way to turn an encrypted database into a plain one, which put in the encrypted file's place opens without a key or a password. Auto-increment counters and the stored schema come across as they are. `plain` beside a `key` or a `password` fails with `INVALID_ARGUMENT`.

### Changed

- A query sorted by an indexed field, with an offset and nothing of its filter left to test on the objects it reads, skips the objects before its page by their keys, rather than reading each of them. A page of 50 objects after up to 98,703 others took 0.69 ms rather than 19 ms on an Apple M1 Max.
- A query whose filter has terms that indexes answer alone gathers the keys of the objects that meet them from the indexes, and combines them before it reads an object, when counting the indexes' entries says that is cheaper: terms joined by `AND` meet, terms joined by `OR` join, a test through a link field with an index, such as `author.country == "kr"`, goes through the linked collection's index and then the link's, and a query sorted by an indexed field with a limit walks that index and takes only the gathered keys. In the benchmark's complex suite on an Apple M1 Max, a count with `OR` across two indexes took 92% less time, a count with conditions on two indexes 69% less, three conditions on two indexes sorted with a limit 39% less, the first 20 posts of authors in one country 93% less, a count through two links 93% less, and the first 20 posts with a tag sorted by another index 95% less.
- A set of gathered keys that are all ints lying close together, as an auto-increment collection's keys are, is looked up through a bit for each int rather than a table of their hashes, and its keys are read once to find their order and their range. In the benchmark's complex suite on an Apple M1 Max, against the sets of keys above, the count with conditions on two indexes took 40% less time, the count through two links 33% less, the count with `OR` 27% less, three conditions sorted with a limit 19% less, the first 20 posts of authors in one country 17% less, and the first 20 posts with a tag 7% less.
- `CONTAINS` on a string searches the string's bytes with a searcher made once for the query, and checks the string is UTF-8 only where it finds the text, rather than checking and searching each string anew. A query that counts the posts whose text contains two words in a row, over 300,000 posts with no index, took 11% less time on an Apple M1 Max. The engine depends on the `memchr` crate, MIT OR Unlicense, for the search.
- `update` of fields that hold scalars changes the record where it lies in a collection with lists, embedded objects or links too, copying their values as they are, rather than reading the object and writing it whole again.

## v1.3.0 (2026-10-13)

### Added

- A database opened without a schema works with the one the file holds, as it is: its collections are read and written by name, with the stored schema's checks and indexes kept as ever, and nothing is stored, checked or migrated, so a program can open a file whose schema it does not know. An int reads as a number up to 2^53 and as a `bigint` beyond, since no declaration says which. A file that holds no schema has no collections then, as before.
- `Database.storedSchema`, which describes the schema a database works with in plain data, the declared one or the file's own: its version, and each collection's name, primary key, whether the key is the auto-increment, fields with their types, whether each is optional and its default, and indexes. The types `StoredSchema`, `StoredCollection`, `StoredField`, `StoredIndex` and `StoredType` describe it.

### Changed

- `schemaVersion` gives the version of the file's own schema for a database opened without one, where it gave `null`.
- Reaching a collection of a database without a schema fails with the engine's error, as in every other language: `INVALID_ARGUMENT` when the file holds no schema, or stored one after the database opened, and `CORRUPTED` or `UNSUPPORTED_FORMAT_VERSION` when the one it holds does not read.

## v1.2.0 (2026-10-11)

### Changed

- A sync commit on Linux, Android and the BSDs makes the file durable with `fdatasync` rather than `fsync`, which leaves out the file's times and still syncs its length with its data. A sync commit of one object takes a quarter less time for it on a GitHub-hosted Linux runner, 312 µs against 413. macOS, iOS and Windows sync as before.
- A commit asks the file system for the file's length only when its record counts pages past both the published commit's and the last one the commit wrote, rather than at every commit. A deferred commit of one object takes 11% less time for it on a GitHub-hosted Linux runner, 22.3 µs against 25.1.
- A query that reads every object, as one that tests or sorts by a field without an index does, remembers where that field lies in each record it read, beside the page cache, and the next query that tests or sorts by the same field reads it there. On a GitHub-hosted Linux runner, a query of one value of such a field took a fifth less time the next time it ran, and one of the ten objects with the highest value of such a field a quarter less. A filter that is one test of one field is also made without going through the filter's tree, and compares a short string in place, which takes another tenth off such a query each time it runs. The first run takes a few percent more, and what is remembered takes up to a sixteenth more memory than the page cache's size.
- An equality in a filter that is one test of one field is tested in place, rather than through a call for every object, which takes 6% off a query of one value of a field without an index in the engine.
- A read transaction keeps the root of each collection and index it looks a key up in, and a key it looks up is encoded on the stack when it is a number or a short string. A lookup by primary key or by a unique field takes 8 to 9% less time for it on a GitHub-hosted Linux runner.

### Fixed

- `backup` and `backupAsync` refuse a `passwordHashing` cost out of range with `INVALID_ARGUMENT` before they write anything, as opening does, whether a password is given or not. With a password they failed with `INTERNAL`, and without one they ignored the cost.

## v1.1.0 (2026-10-09)

### Breaking changes

- The file format is version 6, `FORMAT_VERSION`, which earlier releases cannot open. Opening a file of version 5 raises it to version 6 while no other process has it open, so a file this release has opened no longer opens with 1.0, in Node.js or in any other language's package. An application that may go back to an earlier release, or that shares its file with a program still on one, opens with `upgradeFormat: false`, which also creates new files in version 5, and calls `upgradeFormat` once nothing older opens the file.

### Added

- The `upgradeFormat` option of `open` and `openAsync`, and `upgradeFormat` and `upgradeFormatAsync`, which say whether opening raises a file of an older format version, and raise an open one.

### Changed

- An index whose entries for one value come in key order, as a non-unique index's do while the objects' primary keys grow, fills its pages as it goes, where splitting a page in the middle left half of each empty, and so do the branches of every tree. A file of 400,000 sample objects with 14 indexes is 7% smaller for it. Keys in no particular order split pages evenly, as before.
- `compact` and `compactAsync` first write again, full, every tree whose pages inserts left part empty, so that the file ends about as small as a backup's copy: the same file compacts to 119 MiB rather than 140 MiB.
- File format 6 writes the lengths in a leaf's entries as varints rather than in five bytes. A file of 400,000 sample objects with 14 indexes takes 8% fewer pages, 126 MiB against 138, and compacts to 110 MiB against 119, with reads and writes as fast as before. A file raised from version 5 has only its header rewritten; its leaves take the new layout as writes change them, and `compact` rewrites the trees where that saves room.
- The native addon is built with its loops aligned to 64 bytes, so that where a hot loop lands no longer depends on the rest of the build: in a development build, the position of one loop alone made one-object deferred commits 15% slower. It is about 7% larger for it. The build for 64-bit Arm Windows is the exception, since the compiler cannot write that platform's unwind information for a function with padding inside it.

### Fixed

- `compact` and `compactAsync` no longer grow the file when a value in an overflow run near the file's end finds no run of free pages below to move into. The run stays where it is, and a round of moves that would grow the file is dropped.

## v1.0.1 (2026-10-07)

### Fixed

- The copy of a key that `open`, `openAsync`, `salvage` and `backup` make on the way to the engine is kept in memory the package wipes, as `setKey` already did.

## v1.0.0 (2026-10-05)

### Added

- `Database.open` opens a database file, creating it when nothing exists at the path, with `create` and `pageSize` options. `path`, `isOpen`, `pageSize` and `formatVersion` describe the open database, and `close` flushes and closes it.
- Every error thrown by the package carries the engine's stable `code`, such as `NOT_FOUND`, `NOT_A_DATABASE` or `CLOSED`.
- Several processes can open one database file at once, coordinated through the engine's file locks. A database on a network file system is refused with `UNSUPPORTED_FILE_SYSTEM`.
- `engineVersion()` and `FORMAT_VERSION` report the engine inside the package and the file format version it reads and writes.
- Collections of objects under a schema declared with `t`, `collection` and `schema`, passed to `Database.open` as `schema`. TypeScript infers every object's type from the declaration. `db.read(fn)` and `db.write(fn)` run a function in a transaction, committing a write when the function returns and aborting it when it throws, with `durability: 'deferred'` to commit without waiting for the disk. A collection offers `get`, `find`, `findOne`, `count`, `insert`, `insertMany`, `put`, `putMany` and `delete`, and objects cross into the engine as records, a batch in one call. A property the schema does not have is refused, and so is a modifier on a list's element, such as `t.list(t.string().index())`, rather than dropped: an index goes on the list itself. `t.bigint()` declares an int read as a `bigint`, write transactions on one file do not nest, even through two `Database` objects or from a migration function, and `busyTimeout` sets how long a write waits for another process.
- Queries built with `where`, `sortBy`, `limit` and `offset`, typed by the collection's fields, or written in the query language with parameters. `conditions` and `Query` build filters outside a collection.
- The `cacheSize` option of `Database.open` and `Database.openAsync` sets how much memory the page cache of a file may take, in bytes: 32 MiB by default.
- Prepared queries: `db.prepare(collection, query)` parses a query once, as text or built with `param` in place of values, and `find`, `findOne` and `count` run it with its parameters' values, in synchronous and asynchronous transactions. The package also keeps up to 256 texts it has parsed, so a text query run again with other parameters is not parsed again.
- Migrations: raising the schema's version migrates the file when it opens, with the renames, deletions and replaced fields a migration lists and a JavaScript function that reads the objects as they were through `previous` and `previousKeys`.
- `db.schemaVersion`, and `db.sync()` to make deferred commits durable.
- The package is written in TypeScript, and its type declarations are generated from its source. They name the types of `t`, `Database` and `Query` as `TypeBuilders`, `DatabaseOpener` and `QueryConstructor`.
- An asynchronous API: `Database.openAsync`, `readAsync`, `writeAsync`, `syncAsync` and `closeAsync` do the engine's work on the libuv thread pool and resolve promises, so the event loop never waits for the disk or for another process's writer. Their transaction functions and migration functions may be asynchronous, every collection method returns a promise, and a transaction runs its operations in the order they were called and commits once they have all settled. Operations called together, or while earlier ones are on the pool, cross into the engine as one batch in one trip, and `readAsync` begins its read on the calling thread, since a read waits for no writer. This process's writes on one file queue in JavaScript rather than on the pool, however each handle named the file, a hard link on Windows included, `syncAsync` and `closeAsync` with them, and a synchronous `write`, `sync` or `close` while an asynchronous write holds the file fails with `INVALID_ARGUMENT` rather than blocking the event loop.
- `update(key, changes)` on the collections of synchronous and asynchronous write transactions sets the fields it is given, keeps the rest, and says whether there was an object. `null` makes an optional field null and gives a field with a default its default, and a field left `undefined` stays as it is. Only the changed fields cross into the engine, which changes the stored record where it lies, so updating a field costs less than reading the object and putting it back.
- `db.check()` and `db.checkAsync()`, the integrity check of the published commit, which report every problem they find, each with its page and its tree or collection, rather than throwing.
- `db.backup(path)` and `db.backupAsync(path)`, a copy of the published commit in a new file, which the same key or password opens and which never replaces a file already at the path. A `key` or a `password` in their `BackupOptions` encrypts the copy under a new data key instead, which is how a file's data key changes, and how a plain database's copy is encrypted.
- `db.compact()` and `db.compactAsync()`, which make the file smaller in place. Like a write, `compactAsync` waits its turn after this process's other writes on the file, and `compact` is refused while one of them holds it.
- `Database.salvage(from, into)` and `Database.salvageAsync(from, into)`, which rescue what they can of a damaged file into a new one, and report what they rescued and what they could not. They work on a file that does not open, need it alone, so a file in use fails with `BUSY`, and never replace a file already at `into`.
- Encryption: `key` or `password` in the options of `open`, `openAsync` and `Database.salvage` encrypts a new database or opens an encrypted one, `passwordHashing` sets the cost of hashing a password, `db.isEncrypted` tells the two kinds of file apart, and `setKey`, `setPassword` and their `Async` twins change the key. Opening an encrypted database without its key fails with `KEY_REQUIRED`, and with another one with `WRONG_KEY`.
- Electron's main process is supported, and the tests run the package there, in Electron 44. The guide says how several Electron processes share one file and how to keep the addon outside an `asar` archive.
- The prebuilt addons ship as optional dependencies under the `@darudb` scope, one package per platform, such as `@darudb/darudb-linux-x64-gnu`, and npm installs only the one for the platform it runs on. Every platform package ships `LICENSE` and `THIRD_PARTY_NOTICES.txt`, the licences of the Rust crates and of the C source linked into its addon.
