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

Every database is used the way an application in that language would use it. DaruDB runs through its public API with a schema, SQLite through SQL with prepared statements, and the key-value stores keep a record and the indexes an application would write for them. Every read is made into an object of the language with all its fields, and a query that runs many times is prepared once.

## How it is kept fair

- **The same durability.** A sync commit waits until the disk has it: DaruDB's default commit, SQLite's write-ahead log with `synchronous = FULL`, LMDB's default commit, redb's `Durability::Immediate`, and Hive's write followed by `flush`. A deferred commit returns before that: DaruDB's deferred commit, `synchronous = NORMAL`, LMDB's `NO_SYNC`, redb's `Durability::None` and Hive's write alone. redb's promises less than the others, since nothing it writes is durable until a later immediate commit.
- **The same work.** Every row hashes the keys and ages of what it found, in the order it found them, and a run in which two databases disagree fails rather than giving a time.
- **A clean start.** Each pass of each database runs in a process of its own, on new files, and the databases take turns in an order that changes every run. A time is the median of the runs.
- **The same machine.** The databases of one language run on one machine, one after another.

The cache each database may use is left at its default, apart from SQLite's, which is raised to DaruDB's 32 MiB. A database that keeps every object in memory, as Hive does, reads without touching its file.

## Where DaruDB is behind, and why

DaruDB is ahead in most rows of every language. These are the rows where it is not, with what makes each slower:

- **Sync commits** land within a few percent of each other, since waiting for the disk is nearly all of one. On Apple systems, Hive's `flush` is an `fsync`, which stops short of the disk's own cache, so its sync commit there takes a fraction of the others' time because it promises less.
- **A deferred commit of one object.** LMDB is ahead wherever it runs, and so is redb in Rust, whose deferred commit promises less. A DaruDB commit computes the check of every page it writes, and because processes coordinate through file locks alone, it takes the writer lock, reads the header again and releases the lock. LMDB writes its pages without a check and coordinates processes through a mutex in shared memory, which DaruDB rejects: a process that dies holding one can leave the others stuck.
- **Queries without an index.** LMDB in Rust and Hive in Dart scan faster, and redb finds the ten highest scores a little faster. The record written by hand for LMDB keeps the age and the score at fixed places, so a scan reads those bytes and decodes only what it returns, and Hive keeps every object in memory, decoded already. A DaruDB record names each field with an id and a type, which makes renaming a field free and lets the engine check what it reads from a file it does not trust, so a scan steps over the fields before the one it tests, and it reads each leaf through the page cache. Through Node.js and Python, the other stores' own decoding costs more than that, and DaruDB is ahead.
- **Reads in Rust.** redb finds one object by its key faster: what DaruDB spends there is the search inside each node of the tree and the strings the struct allocates. LMDB and redb also read the sorted range faster. The query sorts by age descending and then by key, so DaruDB walks the age index backwards and holds back the objects of each age to give them in key order, where the loop written for the key-value stores asks for each age in turn.
- **A lookup through a unique index in Node.js**, where SQLite is a few percent ahead: the first lookup that reaches a page of an index loads the page, checks it and works out the heads of its keys.
- **Rows within 5% either way**, such as SQLite's ten highest scores in Node.js and Dart, are within what one run moves on a machine busy with other work.

## Run it yourself

The harness is in [`bench/`](https://github.com/jooy2/darudb/tree/main/bench) of the repository, with a README that lists every setting. The `run-benchmarks` workflow runs it on GitHub-hosted runners and opens a pull request with the results for this page.

```bash
cd bench/rust && cargo build --release && cd ../..
node bench/run.mjs rust --runs 5 --out bench/results/rust.json
```
