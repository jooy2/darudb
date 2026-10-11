---
title: Performance
order: 3
aside: false
pageClass: performance-page
---

# Performance

How fast DaruDB reads and writes next to the embedded databases each of its languages uses most, measured with the benchmark in the repository: the same objects and workloads in every database, at the same durability settings, on one machine.

The table follows the language chosen in the sidebar. The stores of one language ran on one machine one after another, so their times compare with each other; the tables of different languages come from different runs and do not.

<PerformanceTable />

## What is measured

One collection of 100,000 objects, each with a name, an email under a unique index, an age under an index, a city and a score, both without an index. Every row times many operations and divides by their number:

| Workload | What it does |
| --- | --- |
| Insert, one object per sync commit | 500 commits of one object, each waiting for the disk |
| Insert, one object per deferred commit | 10,000 commits of one object, each leaving the sync for later |
| Insert 100,000 objects in one transaction | Every object, the commit included |
| Get by primary key, random order | 100,000 reads of one object |
| Get by a unique index, random order | 20,000 reads of one object through the email |
| Query an indexed value, 1,250 objects | The objects of one age |
| Query an indexed range, sorted, first 20 | The first 20 objects of five ages, sorted by age descending, then by key |
| Count through an index | The objects with an age of 40 or more |
| Query without an index, 1,000 objects | The objects of one city, ten times over |
| Query without an index, 1,000 objects, first run | The first of those ten |
| Top 10 by a field without an index | The 10 objects with the highest score, ten times over |
| Top 10 by a field without an index, first run | The first of those ten |
| Read and update 10,000 objects | Each read, given a new age and written back, in one transaction with its commit |
| Delete 10,000 objects | In one transaction with its commit |

The two queries without an index each have a row of their own for their first run, since DaruDB answers such a query faster once it has run: it remembers where the field it tests or sorts by lies in each record it read, and reads it there the next time. A query that an application runs once takes the time of the first run.

Every database is used the way an application in that language would use it. DaruDB runs through its public API with a schema, SQLite through SQL with prepared statements, and the key-value stores keep a record and the indexes an application would write for them. Every read is made into an object of the language with all its fields, and a query that runs many times is prepared once. An update sets the one field it changes where the language's DaruDB package can, as SQL sets one column: through Node.js and Python with `update`, where writing the whole object back would build it again in the language first, and through Rust and Dart by writing the typed object back.

The Node.js table also has Realm, for applications moving from it, though its vendor deprecated it in September 2024 and ended support in September 2025; the [migration guide](./migration/realm.md) shows how to move the data. Realm assigns no keys, so the application gives each object the key the other stores assign. It has no unique index besides the primary key, so the email has a plain index, and its queries are strings it parses on every call, since it has no prepared queries. An update sets the one property.

## How it is kept fair

- **The same durability.** A sync commit waits until the disk has it: DaruDB's default commit, SQLite's write-ahead log with `synchronous = FULL`, LMDB's default commit, redb's `Durability::Immediate`, Hive's write followed by `flush`, and Realm's commit. A deferred commit returns before that: DaruDB's deferred commit, `synchronous = NORMAL`, LMDB's `NO_SYNC`, redb's `Durability::None` and Hive's write alone. Realm has no commit that leaves the sync for later, so its cell in that row is empty. redb's promises less than the others, since nothing it writes is durable until a later immediate commit.
- **The same work.** Every row hashes the keys and ages of what it found, in the order it found them, and a run in which two databases disagree fails rather than giving a time.
- **A clean start.** Each pass of each database runs in a process of its own, on new files, and the databases take turns in an order that changes every run. A time is the median of the runs.
- **The same machine.** The databases of one language run on one machine, one after another.

The cache each database may use is left at its default, apart from SQLite's, which is raised to DaruDB's 32 MiB. A database that keeps every object in memory, as Hive does, reads without touching its file.

The tables come from the `run-benchmarks` workflow, which runs each language on a GitHub-hosted Linux runner of its own. Those runners mount their disk without write barriers, so a sync commit there waits for the file system's work but not for the disk's own cache. They also differ in processor and disk from one run to the next, and the commit rows move most with them: through Node.js, a deferred commit of one object took SQLite 68 µs and DaruDB 34 µs on the runner of one run, and 34 µs and 46 µs on that of the next. The next section says why. Compare a commit row's databases within one table only.

## Next to Realm

For an application moving from Realm, the Node.js table is the one to read; choose Node.js in the sidebar to see it. DaruDB is ahead of Realm in every row but the query of one city, a field without an index. Realm keeps each property's values together and reads only the city, and takes 0.67 times DaruDB's time on the first run; over ten runs the two are level, since from the second run on DaruDB reads the city where the first found it in each record. Realm takes 2.2 times as long for a sync commit and has no deferred one, 6.7 times as long to insert 100,000 objects in one transaction, and 6.5 times to delete 10,000. It takes 3.9 times as long to read by key, and 13 times through the email, since it parses a query string on every call and has no unique index besides the primary key. It takes 4.8 times as long for the objects of one age, 115 times for the first 20 of five ages, since it sorts every object of the five ages before it takes the first 20, and 2.6 to 3.7 times for the ten objects with the highest score. An update takes about as long in both, since both set one field.

## Where DaruDB is behind, and why

In this run Node.js and Dart ran on runners with AMD EPYC 7763 processors, Rust on one with an AMD EPYC 9V74, and Python on one with an Intel Xeon 6973P-C. DaruDB is ahead of every other database in most rows in Node.js, Dart and Python, and in Python ahead of SQLite in every row but one. In Rust, LMDB is ahead in most rows, and redb in the deferred commit, the reads by key and the queries. These are the rows where DaruDB is not ahead, with what makes each slower:

- **A deferred commit of one object.** LMDB is ahead in every language it runs in, and SQLite in Node.js and Dart, by 22 to 25%. On Linux, about half of DaruDB's commit is spent in the kernel: it makes about fourteen system calls, ten writes for the eleven or so pages it writes and four to take and release the writer lock and to check for other writers and readers, since processes coordinate through file locks alone. LMDB makes seven writes and coordinates processes through a mutex in shared memory, which DaruDB rejects: a process that dies holding one can leave the others stuck. SQLite's deferred commits are not all of its work: it syncs its log and its file at each checkpoint of the log, every thousand pages, and on most runners its deferred commits spent two thirds of their time or more waiting for those syncs. The EPYC 7763 runners finish them sooner, and their processor, the oldest of the runner kinds, is the slowest at DaruDB's commit, which waits for nothing; on the other kinds of runner, DaruDB's deferred commit took half of SQLite's time or less, as it did in Rust and Python in this run.
- **Sync commits** move from pass to pass more than any other row: LMDB is 18% ahead in Rust in this run, while DaruDB is ahead in Python. On the runners, a sync commit is mostly the file system's work. DaruDB writes a commit's pages where the free pages are, scattered over the file, where SQLite appends its pages to its log, and on the EPYC 7763 runners syncing twelve pages written scattered over a file took three times as long as syncing the same twelve written in one run, in a probe of the disk alone.
- **Reads and queries through an index, in Rust.** LMDB and redb find an object by its key 19 to 26% faster, LMDB finds one by its email 26% faster, and both run the queries through the age index faster, as SQLite does by 11 to 12% on this runner. A DaruDB lookup finds each page below the root in its page cache, under a lock and with a reference count, which takes a larger share of a lookup on these x86 processors than on Apple's, then searches the node comparing keys as bytes, and decodes the record, which tags each field with an id, into the struct. LMDB reads its file through a memory map, with no cache to look in, redb compares its primary keys as integers, and both read the record written by hand for them, with its fields at fixed places. DaruDB does not map its file, because another process can change mapped bytes under a reader, and a mapping does not mix with encryption. The sorted range costs more besides: it sorts by age descending and then by key, so DaruDB walks the age index backwards and holds back the objects of each age to give them in key order, where the loop written for the key-value stores asks for each age in turn. On an Apple M1 Max, DaruDB was ahead of LMDB in these rows but the sorted range.
- **Queries without an index.** LMDB in Rust scans 2.4 to 7 times as fast, redb in Rust 1.2 to 2 times, Hive in Dart, which keeps every object in memory, 1.7 to 3.2 times, and Realm in Node.js finds the objects of one city faster on the first run. SQLite is ahead on the first run alone, by 11 to 18% in Rust, Node.js and Dart and in one of the two queries in Python, and DaruDB is ahead of it over ten runs, by 22 to 49%. The record written by hand for the key-value stores keeps the age and the score at fixed places, so a scan reads those bytes and decodes only what it returns; Realm keeps each property's values together, so a scan reads only the city; and SQLite's record lists the types of its columns at its start, so finding one column adds up the sizes before it without reading them. A DaruDB record tags each field with an id and a type, which makes renaming a field free and lets the engine check what it reads from a file it does not trust, so a first scan steps over the fields before the one it tests in each record. It remembers where it found the field, and the runs after the first read it there.
- **A lookup through a unique index in Node.js**, where SQLite is 10% ahead: two lookups make it, the email's entry in the index and then the object by its key, and making the object from its record in JavaScript takes a fifth of it.
- **Rows within 8% either way**, such as SQLite's sync commits in Node.js and Dart and its delete in Dart, and LMDB's update in Rust, are within what one run moves on a shared runner.

## Run it yourself

The harness is in [`bench/`](https://github.com/jooy2/darudb/tree/main/bench) of the repository, with a README that lists every setting. The `run-benchmarks` workflow runs it on GitHub-hosted runners and opens a pull request with the results for this page.

```bash
cd bench/rust && cargo build --release && cd ../..
node bench/run.mjs rust --runs 5 --out bench/results/rust.json
```
