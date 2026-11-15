# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- Searching text for words, with text indexes. `F.body.matches("quick fox")`, and `body MATCHES "quick fox"` in the query language, find the objects whose text has every word, in any order and ignoring case; a word followed by `*` is the start of a word, so `"data*"` finds `database`, and the search can be a parameter, `param(0)` or `$0`. A word is a run of letters, digits and marks, known by its first 64 characters; Chinese characters, hiragana and katakana, written without spaces, are taken two at a time, so `"東京"` finds a text with those two characters side by side. A search without a word finds nothing, a list holds when one of its strings has every word, and nothing ranks the results or tests that words stand together. `darudb.field(text_index=True)` on a `str` field or a `list[str]`, beside `index=True` if the field has one, and `darudb.index(F.meta.note, text=True)` in `@darudb.collection(indexes=[...])` for a field inside an embedded class, declare a text index, which answers a search without reading the objects; without one every object is read and searched. A text index is on one field and is never unique, which `darudb.index` refuses with `INVALID_ARGUMENT` at once, and a field that holds no text is refused when the database opens. `StoredIndex.text` reads the declaration back. A text index holds an entry for each different word of each object, so a long text costs more to write than any other field. A file with a text index is in object format 3, which earlier releases do not open; `upgrade_format=False` refuses a schema that adds one with `INVALID_ARGUMENT`.
- `get_many(keys)` on a collection, and on one of the asynchronous API: the objects of several primary keys, in the order of the keys, with None where no object has a key, read in one call into the engine.
- Read-only databases: `read_only=True` to `open` and `open_async` opens a database through which the file never changes, and `is_read_only` says so. It reads, checks and backs up; a write block, `compact`, `upgrade_format`, a change of key and their `_async` twins fail with the new code `READ_ONLY`, and so does opening with a schema the file does not hold already. Nothing is created, and a file that needs recovery, left by a process that died or by a power cut, is refused with `READ_ONLY` until it has been opened once without the option. The process still needs write access to the file; another `Database` of the file writes beside it as ever.
- `max_unsynced_pages` and `max_unsynced_time`, options of `open` and `open_async`: how many pages deferred commits may write, and how long they may wait, in seconds, before one of them is made durable anyway. 16384 pages and one second by default, as before.
- `commit_id` on a read transaction, and on one of the asynchronous API: the id of the commit the transaction sees, which only grows, so a larger one than an earlier transaction saw says that something was committed in between, by this process or another.

### Changed

- The page cache of a file takes up to 128 MiB by default on Windows, macOS and Linux, where it took 32 MiB, and `cache_size` takes at most 256 MiB there. On iOS and Android both stay 32 MiB. A value above the most is taken as the most. The cache fills only as pages are read and is in memory alone, so a file smaller than it takes no more than before; a scan of a file larger than 32 MiB no longer reads it from the disk each time.

### Fixed

- A call of an asynchronous transaction that is cancelled while it runs, by `asyncio.wait_for` for one, ends before the cancellation is raised, and so does a commit. The transaction went on while the call still ran on the pool, and leaving the block then waited for it on the event loop's thread, which stopped the loop for as long as the call took. What a cancelled call did is part of the transaction, and a cancelled commit has committed.
- A `Query` object kept for the life of a program no longer keeps every `Schema` it ran under alive: it holds its compiled form for at most eight collections and schemas.
- A list that holds itself, given as a value of a list field or as a query's value, is refused with `INVALID_ARGUMENT`: a list holds no list. Converting it went into the list without end and ended the process.
- `F.tags`, and `F` itself, are not iterable. `list(F.tags)`, or `F.tag.is_in(F.tags)`, never ended.
- A `default_factory` that gives another value each time it is called is refused with `INVALID_ARGUMENT` when the schema is made. The default is part of the stored schema, so the program's next run failed to open the file with `SCHEMA_MISMATCH`.
- A dict written to a collection reached by name is read as it is when the write begins, so one that changes while its values are converted no longer raises a `PanicException`.
- An `async with db.write_async()` block left in another task than the one that entered it gives the file back. Closing an asynchronous generator that was suspended inside such a block raised `ValueError` before the transaction ended, and every later asynchronous write, sync or close on the file was refused or waited for good.
- A task made inside an asynchronous write, or inside a migration's function, may write to the file once that write is over. It was refused with `INVALID_ARGUMENT` for as long as it lived.
- A `with db.write()` block that fails as it is entered, after its transaction began, ends the transaction and no longer leaves the file marked as held by the thread.

## v1.5.0 (2026-11-01)

### Breaking changes

- `StoredType.kind`, in what `Database.stored_schema` gives, may be `"date"`. Code that handles every kind, such as a `match` that a type checker holds to be exhaustive, needs a case for it.

### Added

- Cascading links: `darudb.field(link=User, cascade=True)` makes deleting the object the link names delete the object that holds the link, and so on down the chain, cycles included, in the same transaction, through `delete`, `delete_where`, their asynchronous forms and a migration's function. `delete` still returns whether the object it was given was there, and `delete_where` how many objects its query found. Only a link to one object, in a field of the collection itself, cascades. An index on the field is how a delete finds the objects to take with it: without one, each delete reads the whole collection that links. `StoredField.cascade` reads the declaration back. A file with a cascading link is in object format 2, which earlier releases do not open.
- Indexes on several fields, and on fields inside embedded objects: `@darudb.collection("people", indexes=[darudb.index("last", "first", unique=True), darudb.index(F.home.city)])`. `darudb.index` takes 1 to 8 fields, each an attribute's name, a path such as `"home.city"` or an `F` path, which turn into the names the file stores. A query testing the first fields for equality and the next for a range reads one range of its entries, and one that sorts by the fields after them gets its objects in order. A unique one refuses two objects with the same values in every field, though any number may hold `None` in one. `StoredIndex` has `fields`, every field an index is on. Such an index needs object format 2.
- Lists of embedded objects: a field annotated `list[Stop]`, where `Stop` is an `@darudb.embedded` class, holds a list of them and reads back as a list of instances. A path through it, `F.stops.city`, tests that field of each element, and a condition holds when it holds for any element; the attributes turn into the names the file stores, as they do through an embedded object. Such a list has no index and no sort, and its default is the empty list, `field(default_factory=list)`. It needs object format 2, as a date does.
- `datetime` fields: a date, kept to the microsecond. A field annotated `datetime` takes an aware `datetime`, whatever its time zone, and reads back as one in UTC; a naive `datetime` names no instant and is refused with `INVALID_ARGUMENT`. The conversion is whole `timedelta` arithmetic, never a float timestamp. A date can be a list's element, a default, an indexed field and a sort's, a query compares it with an aware `datetime`, in a condition, a parameter or `DATE "2026-10-30T12:00:00Z"` in the query language, and `min` and `max` give one. A date beyond the years a `datetime` holds, 1 to 9999, fails with `INVALID_ARGUMENT` when it is read. The first date a schema declares moves the file to object format 2, which releases of the package before this one do not open; `upgrade_format=False` refuses that with `INVALID_ARGUMENT`.
- `darudb.linked(Post, "author", condition)` holds for an object that some object of the collection `Post` links to through its link field `author`, and that meets `condition` on `Post`'s fields, if it is given: `users.find(darudb.linked(Post, "author", F.year >= 2020))`. `~` of it finds the objects nothing links to. The collection may be a class or a name; the link and the condition name its attributes, which the package turns into the names the file stores. It is `ANY posts.author (year >= 2020)` in the query language.
- Tests that ignore case: `Condition.ignore_case()` makes every test of `==`, `!=`, `is_in`, `contains`, `startswith` and `endswith` inside a condition compare strings by Unicode's simple case folding, as `IGNORE CASE` does in the query language: `users.find((F.name == "kim").ignore_case())`. A test that orders, or one of a field holding no strings, fails with `INVALID_QUERY`. Every wheel's `THIRD_PARTY_NOTICES.txt` carries the licence notice of the Unicode data the folding comes from.
- `delete_where` and `update_where` on a write collection, and their `async` twins, delete or update every object a query finds, after its sort, offset and limit, and return how many there were: `users.update_where(F.age < 18, active=False)`. The query is required, in any form `find` takes, with its parameters after it; `Query()` finds every object. An update by query is all or nothing: when one object is refused, by a unique value taken or an index entry too long, the objects changed before it are put back. It cannot set the primary key.
- Aggregates: `sum`, `min`, `max` and `avg` on a collection, and their `async` twins, work out one value from a field of the objects a query finds, after its sort, offset and limit, with the query in any form `find` takes: `users.sum("age", F.age > 18)`, `users.max(F.address.city, "age > $0", 18)`. The field is an attribute's name, a path through embedded objects and links, or an `F` path, and goes to the name the file stores. Nulls are left out; a sum of no values is 0 and the rest are `None`. A sum is exact, an `int` for an int field and a `float` for a float field, and a mean is a `float`.

### Changed

- The native module is built with its functions aligned to 64 bytes as well as its loops, so that where a hot function lands no longer depends on the rest of the build: in the engine's benchmark on an Apple M1 Max, a count through an index had run 12% slower after changes that left its code as it was, and it no longer does. It is about 3% larger for it.

## v1.4.0 (2026-10-29)

### Added

- `plain` in `Database.backup` and `Database.backup_async`, which writes the copy with no encryption: the way to turn an encrypted database into a plain one, which put in the encrypted file's place opens without a key or a password. Auto-increment counters and the stored schema come across as they are. `plain` beside a `key` or a `password` fails with `INVALID_ARGUMENT`.

### Changed

- A query sorted by an indexed field, with an offset and nothing of its filter left to test on the objects it reads, skips the objects before its page by their keys, rather than reading each of them. A page of 50 objects after up to 98,703 others took 0.69 ms rather than 19 ms on an Apple M1 Max.
- A query whose filter has terms that indexes answer alone gathers the keys of the objects that meet them from the indexes, and combines them before it reads an object, when counting the indexes' entries says that is cheaper: terms joined by `AND` meet, terms joined by `OR` join, a test through a link field with an index, such as `author.country == "kr"`, goes through the linked collection's index and then the link's, and a query sorted by an indexed field with a limit walks that index and takes only the gathered keys. In the benchmark's complex suite on an Apple M1 Max, a count with `OR` across two indexes took 92% less time, a count with conditions on two indexes 69% less, three conditions on two indexes sorted with a limit 39% less, the first 20 posts of authors in one country 93% less, a count through two links 93% less, and the first 20 posts with a tag sorted by another index 95% less.
- A set of gathered keys that are all ints lying close together, as an auto-increment collection's keys are, is looked up through a bit for each int rather than a table of their hashes, and its keys are read once to find their order and their range. In the benchmark's complex suite on an Apple M1 Max, against the sets of keys above, the count with conditions on two indexes took 40% less time, the count through two links 33% less, the count with `OR` 27% less, three conditions sorted with a limit 19% less, the first 20 posts of authors in one country 17% less, and the first 20 posts with a tag 7% less.
- `CONTAINS` on a string searches the string's bytes with a searcher made once for the query, and checks the string is UTF-8 only where it finds the text, rather than checking and searching each string anew. A query that counts the posts whose text contains two words in a row, over 300,000 posts with no index, took 11% less time on an Apple M1 Max. The engine depends on the `memchr` crate, MIT OR Unlicense, for the search.

## v1.3.0 (2026-10-13)

### Added

- A database opened without a schema works with the one the file holds, as it is: its collections are reached by name, and their objects are read and written as `dict`s by the names the file stores, with the stored schema's checks and indexes kept as ever. A float field takes an int and an embedded object a `dict`, as through a class. Nothing is stored, checked or migrated, so a program can open a file whose schema it does not know. A file that holds no schema has no collections then, as before.
- `Database.stored_schema`, which describes the schema a database works with, the declared one or the file's own, by the names the file stores: its version, and each collection's name, primary key, whether the key is the auto-increment, fields with their types, whether each is optional and its default, and indexes. The frozen dataclasses `StoredSchema`, `StoredCollection`, `StoredField`, `StoredIndex` and `StoredType` hold it, and it is still there after the database closes.

### Changed

- `schema_version` gives the version of the file's own schema for a database opened without one, where it gave None.
- Reaching a collection of a database without a schema fails with the engine's error, as in every other language: `INVALID_ARGUMENT` when the file holds no schema, or stored one after the database opened, and `CORRUPTED` or `UNSUPPORTED_FORMAT_VERSION` when the one it holds does not read. Reaching one by its class there still fails with `INVALID_ARGUMENT`.

## v1.2.0 (2026-10-11)

### Changed

- A sync commit on Linux, Android and the BSDs makes the file durable with `fdatasync` rather than `fsync`, which leaves out the file's times and still syncs its length with its data. A sync commit of one object takes a quarter less time for it on a GitHub-hosted Linux runner, 312 µs against 413. macOS, iOS and Windows sync as before.
- A commit asks the file system for the file's length only when its record counts pages past both the published commit's and the last one the commit wrote, rather than at every commit. A deferred commit of one object takes 11% less time for it on a GitHub-hosted Linux runner, 22.3 µs against 25.1.
- A query that reads every object, as one that tests or sorts by a field without an index does, remembers where that field lies in each record it read, beside the page cache, and the next query that tests or sorts by the same field reads it there. On a GitHub-hosted Linux runner, a query of one value of such a field took a fifth less time the next time it ran, and one of the ten objects with the highest value of such a field a quarter less. A filter that is one test of one field is also made without going through the filter's tree, and compares a short string in place, which takes another tenth off such a query each time it runs. The first run takes a few percent more, and what is remembered takes up to a sixteenth more memory than the page cache's size.
- An equality in a filter that is one test of one field is tested in place, rather than through a call for every object, which takes 6% off a query of one value of a field without an index in the engine.
- A read transaction keeps the root of each collection and index it looks a key up in, and a key it looks up is encoded on the stack when it is a number or a short string. A lookup by primary key or by a unique field takes 8 to 9% less time for it on a GitHub-hosted Linux runner.
- Reading objects, by key and through a query, reads each record straight into an instance of the class, rather than into an object of the engine first that the instance was then built from. An object read through an index takes about two fifths less time on an Apple M1 Max, and a query of 1,000 objects without an index a tenth less.
- `insert`, `put`, `insert_many` and `put_many` write an object of a class whose fields all hold scalars as its record, field by field, rather than as an object of the engine made by the fields' names, which the engine encoded again. Inserting many objects in one write transaction takes 13 to 15% less time on an Apple M1 Max. A class with a list, an embedded object or a link is written as before.
- A write transaction neither imports the module of the asynchronous API nor takes its lock to make sure no asynchronous write holds the file, while none runs in the process, and looks this thread's write transactions up once rather than three times. A one-object write transaction takes about 1.5 µs less for it on an Apple M1 Max.

### Fixed

- `insert_many` and `put_many` of an asynchronous collection raise `DaruError` with `INVALID_ARGUMENT` for a batch that is not iterable, as the synchronous ones do, rather than `TypeError`.
- `backup` and `backup_async` refuse a `password_hashing` cost out of range with `INVALID_ARGUMENT` before they write anything, as opening does, whether a password is given or not. With a password they failed with `INTERNAL`, and without one they ignored the cost.

## v1.1.0 (2026-10-09)

### Breaking changes

- The file format is version 6, `FORMAT_VERSION`, which earlier releases cannot open. Opening a file of version 5 raises it to version 6 while no other process has it open, so a file this release has opened no longer opens with 1.0, in Python or in any other language's package. An application that may go back to an earlier release, or that shares its file with a program still on one, opens with `upgrade_format=False`, which also creates new files in version 5, and calls `upgrade_format` once nothing older opens the file.

### Added

- The `upgrade_format` parameter of `Database.open` and `open_async`, and `upgrade_format` and `upgrade_format_async`, which say whether opening raises a file of an older format version, and raise an open one.

### Changed

- An index whose entries for one value come in key order, as a non-unique index's do while the objects' primary keys grow, fills its pages as it goes, where splitting a page in the middle left half of each empty, and so do the branches of every tree. A file of 400,000 sample objects with 14 indexes is 7% smaller for it. Keys in no particular order split pages evenly, as before.
- `compact` and `compact_async` first write again, full, every tree whose pages inserts left part empty, so that the file ends about as small as a backup's copy: the same file compacts to 119 MiB rather than 140 MiB.
- File format 6 writes the lengths in a leaf's entries as varints rather than in five bytes. A file of 400,000 sample objects with 14 indexes takes 8% fewer pages, 126 MiB against 138, and compacts to 110 MiB against 119, with reads and writes as fast as before. A file raised from version 5 has only its header rewritten; its leaves take the new layout as writes change them, and `compact` rewrites the trees where that saves room.
- The native module is built with its loops aligned to 64 bytes, so that where a hot loop lands no longer depends on the rest of the build: in a development build, the position of one loop alone made one-object deferred commits 15% slower. It is about 7% larger for it. The build for 64-bit Arm Windows is the exception, since the compiler cannot write that platform's unwind information for a function with padding inside it.

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
