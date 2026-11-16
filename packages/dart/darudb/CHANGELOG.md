# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Fixed

- The native library is built with `ctutils` 0.4.8, a crate the ciphers depend on, whose versions before 0.4.4 could compile a constant-time mask into a branch (RUSTSEC-2026-0385).

## v1.6.0 (2026-11-15)

### Changed

- The page cache of a file takes up to 128 MiB by default on Windows, macOS and Linux, where it took 32 MiB, and `cacheSize` takes at most 256 MiB there. On iOS and Android both stay 32 MiB. A value above the most is taken as the most. The cache fills only as pages are read and is in memory alone, so a file smaller than it takes no more than before; a scan of a file larger than 32 MiB no longer reads it from the disk each time.

### Added

- Searching text for words, with text indexes. `matches('quick fox')` on a `StringField` or a `StringListField`, in the generated query builder and in a collection read as maps, and `text MATCHES "quick fox"` in the query language, with the search in the text or in a parameter, find the objects whose text has every word, in any order and ignoring case; a word followed by `*` is the start of a word, so `data*` finds `database`. A word is a run of letters, digits and marks, known by its first 64 characters; Chinese characters, hiragana and katakana, written without spaces, are taken two at a time, so `東京` finds a text with those two characters side by side. A search without a word finds nothing, a list holds when one of its strings has every word, and nothing ranks the results or tests that words stand together. `@TextIndex()` on a `String` or a `List<String>` field, nullable or not, and `@IndexOn(['margin.note'], text: true)` on the class for a field inside an embedded object, keep a text index, which answers a search without reading the objects; without one every object is read and searched. A field may have one beside `@Index()`. `FieldSpec.textIndex` and `IndexSpec.text` declare them and read them back from `Database.storedSchema`. A text index holds an entry for each different word of each object, so a long text costs more to write than any other field. A file with a text index is in object format 3, which earlier releases do not open; `upgradeFormat: false` refuses a schema that adds one with `INVALID_ARGUMENT`.
- `getMany(keys)` on a collection, and on one of the `Future` API: the objects of several primary keys, in the order of the keys, with `null` where no object has a key. It is one call into the library, and with the `Future` API one trip to another thread, where a loop of `get` makes one a key.
- Read-only databases: `readOnly: true` to `Database.open` and `Database.openAsync` opens a database through which the file never changes, and `isReadOnly` says so. It reads, checks and backs up; `write`, `compact`, `upgradeFormat`, `setKey`, `setPassword` and their `Future` forms fail with the new code `READ_ONLY`, and so does opening with a schema the file does not hold already. Nothing is created, and a file that needs recovery, left by a process that died or by a power cut, is refused with `READ_ONLY` until it has been opened once without the argument. The process still needs write access to the file; another `Database` of the file writes beside it as ever.
- `maxUnsyncedPages` and `maxUnsyncedTime`, arguments of `Database.open` and `Database.openAsync`: how many pages deferred commits may write, and how long they may wait, before one of them is made durable anyway. 16384 pages and one second by default, as before.
- `commitId` on a read transaction, and on one of the `Future` API: the id of the commit the transaction sees, which only grows, so a larger one than an earlier transaction saw says that something was committed in between, by this process or another.
- `MigrationContext.version`, the schema version of the step under way, which a function shared by several steps tells them apart by.

### Fixed

- A transaction of the `Future` API whose isolate goes away before it ends, because the isolate was killed or a Flutter app restarted hot while its function was waiting, is freed with the isolate. A write transaction left that way held the file's writer for as long as the process lived, and every later write in the process failed with `BUSY`.
- A call of the `Future` API refused before it reaches the library, as `syncAsync` on a closed database is, no longer keeps the isolate alive: a program that made one did not exit when `main` returned.
- Calls of the `Future` API made at the same moment each get a thread while fewer than 64 are busy. Two calls made before an idle thread woke shared that thread, so the second waited for the first, for the busy timeout when the first was a write waiting for the writer the second belonged to.
- A default of a list of floats written with whole numbers, `const [1, 2]`, is stored as floats, as a float field's own default is. The database failed to open with `INVALID_ARGUMENT`.
- A negative `cacheSize` fails with `INVALID_ARGUMENT`, where it asked for all the memory there is.
- A timer, a `then` or a listener that a `writeAsync` function started may write to the file once that write is over. It was refused with `INVALID_ARGUMENT`, as a write nested in the function is, for as long as it lived.
- A call made from inside another call's function no longer overwrites what that call was encoding. A `count` or `find` made in the function that gives `updateWhere` or `update` its changes, or in a query's function, replaced the outer call's query, so `updateWhere` changed the objects of the inner query; and an iterable given to `insertMany` or `putMany` that inserted as it was read lost objects and stored one twice.
- A transaction that is still running when its database is closed goes on reading and writing safely until its function ends. `close` freed what the transaction's collections name their collection with, so `get`, `insert`, `put`, `update` and `delete` on such a collection read freed memory, which could fail with a wrong error or crash. It happened when `close` or `closeAsync` ran while a `readAsync` function was between two awaits.

## v1.5.0 (2026-11-01)

### Breaking changes

- `Kind` has a new subclass, `DateKind`. An exhaustive `switch` on a `Kind` needs a case for it.

### Added

- Cascading links: `@Cascade()` on a `Link` field, nullable or not, makes deleting the object the link names delete the object that holds the link, and so on down the chain, cycles included, in the same transaction, through `delete`, `deleteWhere`, their `Future` forms and a migration's function. `delete` still returns whether the object it was given was there, and `deleteWhere` how many objects its query found. Only a link to one object, in a field of the collection itself, cascades. An index on the field is how a delete finds the objects to take with it: without one, each delete reads the whole collection that links. `FieldSpec.cascade` declares it and reads it back from `Database.storedSchema`. A file with a cascading link is in object format 2, which earlier releases do not open.
- Indexes on several fields, and on fields inside embedded objects: `@IndexOn(['last', 'first'], unique: true)` beside `@Collection()` on the class, as many as it needs, each on 1 to 8 fields by their Dart names or paths such as `'home.city'`. `CollectionSchema.indexes` holds them as `IndexSpec`s by the names the collection stores, and a collection read as maps reads them back. A query testing the first fields for equality and the next for a range reads one range of entries, and one that sorts by the fields after them gets its objects in order. A unique one refuses two objects with the same values in every field, though any number may hold null in one. Such an index needs object format 2.
- Lists of embedded objects: a `List` of an `@Embedded()` class is a field, written with `FieldSink.objectList` and read with `FieldSource.objectList`, and `EmbeddedListField` reaches the elements' fields in a query, `q.stops.city.equals('Busan')`, where a condition holds when it holds for any element, and sets the list whole with `set`. A collection read as maps reads one as a list of maps. Such a list has no index and no sort, and its default is the empty list. It needs object format 2, as a date does.
- `DateTime` fields: a date, kept to the microsecond, written in UTC whatever the time's zone and read back as a `DateTime` in UTC. `DateKind` declares one, `DateField` tests one in the typed query builder with `equals`, `lessThan`, `between` and the rest, and `MapQuery.dateField(name)` in a collection read as maps, a list of them is a `List<DateTime>`, `min` and `max` give one, and a condition, a parameter or `DATE "2026-10-30T12:00:00Z"` in the query language compares it with a `DateTime`. The first date a schema declares moves the file to object format 2, which releases of the package before this one do not open; `upgradeFormat: false` refuses that with `INVALID_ARGUMENT`. A date beyond what a `DateTime` holds fails with `INVALID_ARGUMENT` when it is read.
- `QueryBuilder.linkedFrom(schema, (q) => q.link, [where])` holds for an object that some object of `schema`'s collection links to through the link field the function picks, and that meets `where`, a condition on that collection's typed fields: `q.linkedFrom(postSchema, (p) => p.author, (p) => p.year.atLeast(2020))`. `~` of it finds the objects nothing links to. It is `ANY posts.author (year >= 2020)` in the query language.
- Tests that ignore case: `ignoreCase: true` on the `equals`, `notEquals`, `isIn`, `contains`, `startsWith` and `endsWith` of a `StringField`, and on the `contains`, `containsAny`, `anyStartsWith` and `anyEndsWith` of a `StringListField`, and `Condition.ignoreCase()` for every such test inside a condition, compare strings by Unicode's simple case folding, as `IGNORE CASE` does in the query language: `q.where(q.name.equals('kim', ignoreCase: true))`. A test that orders, or one of a field holding no strings, fails with `INVALID_QUERY`. The package ships `LICENSE-UNICODE`, the licence notice of the Unicode data the folding comes from.
- `deleteWhere` and `updateWhere` on a write collection, and on one of the `Future` API, delete or update every object a query finds, after its sort, offset and limit, and return how many there were: `users.updateWhere((q) => [q.active.set(false)], (q) => q.where(q.age.lessThan(18)))`. The query is required; `(q) => q` finds every object. `deleteWhereText`, `updateWhereText` and the `Prepared` forms take the query as text or prepared. An update by query is all or nothing: when one object is refused, by a unique value taken or an index entry too long, the objects changed before it are put back. It cannot set the primary key.
- Aggregates: `sum`, `min`, `max` and `avg` on a collection, and on one of the `Future` API, work out one value from a field of the objects a query finds, after its sort, offset and limit: `users.sum((q) => q.age, (q) => q.where(q.age.atLeast(18)))`. The field is picked from the query builder, so it may be a field of an embedded object or of a linked object, and the result has its type: `sum` of an `IntField` is an `int`, of a `FloatField` a `double`, and `min` and `max` are of the field's type or `null`. `avg` is a `double` or `null`. Nulls are left out, and a sum of no values is 0. `sumText`, `sumPrepared` and the like take the query in the query language or prepared.

### Changed

- The native library is built with its functions aligned to 64 bytes as well as its loops, so that where a hot function lands no longer depends on the rest of the build: in the engine's benchmark on an Apple M1 Max, a count through an index had run 12% slower after changes that left its code as it was, and it no longer does. It is about 3% larger for it.

## v1.4.0 (2026-10-29)

### Added

- `plain` in `Database.backup` and `Database.backupAsync`, which writes the copy with no encryption: the way to turn an encrypted database into a plain one, which put in the encrypted file's place opens without a key or a password. Auto-increment counters and the stored schema come across as they are. `plain` beside a `key` or a `password` fails with `INVALID_ARGUMENT`.

### Changed

- A query sorted by an indexed field, with an offset and nothing of its filter left to test on the objects it reads, skips the objects before its page by their keys, rather than reading each of them. A page of 50 objects after up to 98,703 others took 0.69 ms rather than 19 ms on an Apple M1 Max.
- A query whose filter has terms that indexes answer alone gathers the keys of the objects that meet them from the indexes, and combines them before it reads an object, when counting the indexes' entries says that is cheaper: terms joined by `AND` meet, terms joined by `OR` join, a test through a link field with an index, such as `author.country == "kr"`, goes through the linked collection's index and then the link's, and a query sorted by an indexed field with a limit walks that index and takes only the gathered keys. In the benchmark's complex suite on an Apple M1 Max, a count with `OR` across two indexes took 92% less time, a count with conditions on two indexes 69% less, three conditions on two indexes sorted with a limit 39% less, the first 20 posts of authors in one country 93% less, a count through two links 93% less, and the first 20 posts with a tag sorted by another index 95% less.
- A set of gathered keys that are all ints lying close together, as an auto-increment collection's keys are, is looked up through a bit for each int rather than a table of their hashes, and its keys are read once to find their order and their range. In the benchmark's complex suite on an Apple M1 Max, against the sets of keys above, the count with conditions on two indexes took 40% less time, the count through two links 33% less, the count with `OR` 27% less, three conditions sorted with a limit 19% less, the first 20 posts of authors in one country 17% less, and the first 20 posts with a tag 7% less.
- `CONTAINS` on a string searches the string's bytes with a searcher made once for the query, and checks the string is UTF-8 only where it finds the text, rather than checking and searching each string anew. A query that counts the posts whose text contains two words in a row, over 300,000 posts with no index, took 11% less time on an Apple M1 Max. The engine depends on the `memchr` crate, MIT OR Unlicense, for the search.
- `update` of fields that hold scalars changes the record where it lies in a collection with lists, embedded objects or links too, copying their values as they are, rather than reading the object and writing it whole again.

### Fixed

- A synchronous call whose result is small, made after one whose result passed 1 MiB, such as `check` after an `insertMany` of a hundred thousand objects, read its result from memory the library had already freed. On iOS the app crashed; elsewhere the result could be wrong.

## v1.3.0 (2026-10-13)

### Added

- A database opened without a schema works with the one the file holds, as it is. `Database.storedCollection` gives each of its collections as a `MapCollection`, whose objects are maps by the names the file stores, read and written with the stored schema's checks and indexes kept as ever, and queried with `MapQuery`, which names a field by its stored name and says what it holds. A float field takes an `int`, a link a primary key or a `Link`, and a name the schema lacks is refused rather than dropped. A generated schema whose fields are the stored ones reaches its collection too. Nothing is stored, checked or migrated, so a program can open a file whose schema it does not know. A file that holds no schema has no collections then, as before.
- `Database.storedSchema`, the schema a database works with: the declared one, or the file's own as `MapCollection`s, whose `FieldSpec`s describe each field's name, kind, whether it is optional, its default and its index, and whether it is the primary key. It is still there after the database closes.

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

- `MigrationContext.previous` reads a list of links as the keys they hold, as it reads one link, where it gave the package's internal link objects.
- `findText`, `countText`, `findPrepared` and `countPrepared` of an asynchronous collection complete their `Future` with the error when they fail, as every other call of the `Future` API does. A text that does not parse, or a prepared query of another collection, threw before the `Future` was returned.
- `Database.open`, `openAsync`, `backup`, `backupAsync`, `salvage` and `salvageAsync` wipe every copy of the key or password they make on the way to the engine. The buffer the options were written into, which could be left behind when it grew, and the password's UTF-8 bytes stayed in memory until the garbage collector reclaimed them.
- `backup` and `backupAsync` refuse a `passwordHashing` cost out of range with `INVALID_ARGUMENT` before they write anything, as opening does, whether a password is given or not. With a password they failed with `INTERNAL`, and without one they ignored the cost.

## v1.1.0 (2026-10-09)

### Breaking changes

- The file format is version 6, the top-level `formatVersion`, which earlier releases cannot open. Opening a file of version 5 raises it to version 6 while no other process has it open, so a file this release has opened no longer opens with 1.0, in Dart or in any other language's package. An app that may go back to an earlier release, or that shares its file with a program still on one, such as an app extension, opens with `upgradeFormat: false`, which also creates new files in version 5, and calls `upgradeFormat` once nothing older opens the file.

### Added

- The `upgradeFormat` parameter of `Database.open` and `openAsync`, and `upgradeFormat` and `upgradeFormatAsync`, which say whether opening raises a file of an older format version, and raise an open one.

### Changed

- An index whose entries for one value come in key order, as a non-unique index's do while the objects' primary keys grow, fills its pages as it goes, where splitting a page in the middle left half of each empty, and so do the branches of every tree. A file of 400,000 sample objects with 14 indexes is 7% smaller for it. Keys in no particular order split pages evenly, as before.
- `compact` and `compactAsync` first write again, full, every tree whose pages inserts left part empty, so that the file ends about as small as a backup's copy: the same file compacts to 119 MiB rather than 140 MiB.
- File format 6 writes the lengths in a leaf's entries as varints rather than in five bytes. A file of 400,000 sample objects with 14 indexes takes 8% fewer pages, 126 MiB against 138, and compacts to 110 MiB against 119, with reads and writes as fast as before. A file raised from version 5 has only its header rewritten; its leaves take the new layout as writes change them, and `compact` rewrites the trees where that saves room.
- The native library is built with its loops aligned to 64 bytes, so that where a hot loop lands no longer depends on the rest of the build: in a development build, the position of one loop alone made one-object deferred commits 15% slower. It is about 7% larger for it. The build for 64-bit Arm Windows is the exception, since the compiler cannot write that platform's unwind information for a function with padding inside it.

### Fixed

- `compact` and `compactAsync` no longer grow the file when a value in an overflow run near the file's end finds no run of free pages below to move into. The run stays where it is, and a round of moves that would grow the file is dropped.
- The README and the guide name Flutter 3.38.1 as the oldest release the package installs on, where they said 3.38. Flutter 3.38.0 ships a build of Dart 3.10.0 numbered `3.10.0-290.4.beta`, which the package's `sdk: ^3.10.0` has never accepted.
- The package resolves in a Flutter app on Flutter 3.38.1 to 3.44, the oldest releases it supports. It asked for versions of `hooks`, `code_assets` and `native_toolchain_rust` that need `meta` 1.19, which those Flutter releases pin lower, so `flutter pub get` failed there.

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
