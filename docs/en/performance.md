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
| Query without an index, 1,000 objects | The objects of one city |
| Top 10 by a field without an index | The 10 objects with the highest score |
| Read and update 10,000 objects | Each read, given a new age and written back, in one transaction with its commit |
| Delete 10,000 objects | In one transaction with its commit |

Every database is used the way an application in that language would use it. DaruDB runs through its public API with a schema, SQLite through SQL with prepared statements, and the key-value stores keep a record and the indexes an application would write for them. Every read is made into an object of the language with all its fields, and a query that runs many times is prepared once. An update sets the one field it changes where the language's DaruDB package can, as SQL sets one column: through Node.js and Python with `update`, where writing the whole object back would build it again in the language first, and through Rust and Dart by writing the typed object back.

## How it is kept fair

- **The same durability.** A sync commit waits until the disk has it: DaruDB's default commit, SQLite's write-ahead log with `synchronous = FULL`, LMDB's default commit, redb's `Durability::Immediate`, and Hive's write followed by `flush`. A deferred commit returns before that: DaruDB's deferred commit, `synchronous = NORMAL`, LMDB's `NO_SYNC`, redb's `Durability::None` and Hive's write alone. redb's promises less than the others, since nothing it writes is durable until a later immediate commit.
- **The same work.** Every row hashes the keys and ages of what it found, in the order it found them, and a run in which two databases disagree fails rather than giving a time.
- **A clean start.** Each pass of each database runs in a process of its own, on new files, and the databases take turns in an order that changes every run. A time is the median of the runs.
- **The same machine.** The databases of one language run on one machine, one after another.

The cache each database may use is left at its default, apart from SQLite's, which is raised to DaruDB's 32 MiB. A database that keeps every object in memory, as Hive does, reads without touching its file.

The tables come from the `run-benchmarks` workflow, which runs each language on a GitHub-hosted Linux runner of its own. Those runners mount their disk without write barriers, so a sync commit there waits for the file system's work but not for the disk's own cache. They also differ in processor and disk from one run to the next, and the commit rows move most with them: through Node.js, a deferred commit of one object took SQLite 68 µs and DaruDB 34 µs on the runner of one run, and 34 µs and 46 µs on that of the next. The next section says why. Compare a commit row's databases within one table only.

## Where DaruDB is behind, and why

DaruDB is ahead of every other database in most rows in Dart and Python, and in about half of them in Node.js. In Rust, LMDB is ahead in most rows, and redb in the reads and the scans. These are the rows where DaruDB is not ahead, with what makes each slower:

- **Sync commits** move from pass to pass more than any other row, and their order changes with the runner: SQLite is 11 to 15% ahead in Rust and Node.js, the two are level in Dart, and DaruDB is ahead in Python. On the runners, a sync commit is mostly the file system's work. DaruDB writes a commit's pages where the free pages are, scattered over the file, with a write call for each run of them, where SQLite appends its pages to its log.
- **A deferred commit of one object.** LMDB is ahead in every language it runs in, and so is redb in Rust, whose deferred commit promises less. On Linux, about half of DaruDB's commit is spent in the kernel: it makes about fourteen system calls, ten writes for the eleven or so pages it writes and four to take and release the writer lock and to check for other writers and readers, since processes coordinate through file locks alone. LMDB makes seven writes and coordinates processes through a mutex in shared memory, which DaruDB rejects: a process that dies holding one can leave the others stuck. SQLite is ahead in Rust and Node.js, whose runners had AMD EPYC 7763 processors this time; on the other kinds of runner, DaruDB's deferred commit took half of SQLite's time or less. SQLite's deferred commits are not all of its work: it syncs its log and its file at each checkpoint of the log, every thousand pages, and on most runners its deferred commits spent two thirds of their time or more waiting for those syncs. The EPYC 7763 runners finish them sooner, and their processor, the oldest of the runner kinds, is the slowest at DaruDB's commit, which waits for nothing.
- **Reads and queries through an index, in Rust.** LMDB and redb find an object by its key or its email faster, and run the queries through the age index faster. A DaruDB lookup finds each page it passes through in its page cache, under a lock and with a reference count, which takes a larger share of a lookup on these x86 processors than on Apple's, then searches the node comparing keys as bytes, and decodes the record, which tags each field with an id, into the struct. LMDB reads its file through a memory map, with no cache to look in, redb compares its primary keys as integers, and both read the record written by hand for them, with its fields at fixed places. DaruDB does not map its file, because another process can change mapped bytes under a reader, and a mapping does not mix with encryption. The sorted range costs more besides: it sorts by age descending and then by key, so DaruDB walks the age index backwards and holds back the objects of each age to give them in key order, where the loop written for the key-value stores asks for each age in turn. On an Apple M1 Max, DaruDB was ahead of LMDB in these rows but the sorted range.
- **Queries without an index.** LMDB in Rust scans several times faster, redb in Rust about twice as fast, Hive in Dart up to two and a half times, and SQLite 6 to 15% faster in Node.js, Dart and Python. The record written by hand for the key-value stores keeps the age and the score at fixed places, so a scan reads those bytes and decodes only what it returns, and Hive keeps every object in memory, decoded already. SQLite's record lists the types of its columns at its start, so finding one column adds up the sizes before it without reading them. A DaruDB record names each field with an id and a type, which makes renaming a field free and lets the engine check what it reads from a file it does not trust, so a scan steps over the fields before the one it tests, and it reads each leaf through the page cache. In Rust, SQLite's own work costs more than that, and in Node.js and Python, LMDB's decoding does, and DaruDB is ahead of them.
- **Reads by key in Dart**, where Hive is 16% ahead: Hive keeps every object in memory, so a read by key is a lookup in a map, with no page or record to read.
- **A lookup through a unique index in Node.js**, where SQLite is 11% ahead: the first lookup that reaches a page of an index loads the page, checks it and works out the heads of its keys.
- **Updates in Rust**, where LMDB is 23% ahead on this runner, though DaruDB was ahead on the EPYC 9V45 of an earlier run. The row ends with the commit of 10,000 changes spread over the file, whose pages DaruDB writes where earlier commits freed pages, scattered, with many more write calls than pages in runs would take, and checks every page it writes. LMDB writes its pages without a check.
- **Rows within 6% either way**, such as SQLite's queries through the age index in Rust and Python, and rows whose passes spread widely, such as deletes in Rust and sync commits in Dart, are within what one run moves on a shared runner.

## Run it yourself

The harness is in [`bench/`](https://github.com/jooy2/darudb/tree/main/bench) of the repository, with a README that lists every setting. The `run-benchmarks` workflow runs it on GitHub-hosted runners and opens a pull request with the results for this page.

```bash
cd bench/rust && cargo build --release && cd ../..
node bench/run.mjs rust --runs 5 --out bench/results/rust.json
```
