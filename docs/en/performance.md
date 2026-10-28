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

Every database is used the way an application in that language would use it. DaruDB runs through its public API with a schema, SQLite through SQL with prepared statements, and the key-value stores keep a record and the indexes an application would write for them. Every read is made into an object of the language with all its fields, and a query that runs many times is prepared once. An update sets the one field it changes, as SQL sets one column: through Node.js, Python and Dart with `update`, where writing the whole object back would build it again in the language first, and through Rust with `TypedWriter::set`, which names the field through the type.

The Node.js and Dart tables also have Realm, for applications moving from it, though its vendor deprecated it in September 2024 and ended support in September 2025; the [migration guide](./migration/realm.md) shows how to move the data. Realm assigns no keys, so the application gives each object the key the other stores assign. It has no unique index besides the primary key, so the email has a plain index, and its queries are strings it parses on every call, since it has no prepared queries. An update sets the one property.

## How it is kept fair

- **The same durability.** A sync commit waits until the disk has it: DaruDB's default commit, SQLite's write-ahead log with `synchronous = FULL`, LMDB's default commit, redb's `Durability::Immediate`, Hive's write followed by `flush`, and Realm's commit. A deferred commit returns before that: DaruDB's deferred commit, `synchronous = NORMAL`, LMDB's `NO_SYNC`, redb's `Durability::None` and Hive's write alone. Realm has no commit that leaves the sync for later, so its cell in that row is empty. redb's promises less than the others, since nothing it writes is durable until a later immediate commit.
- **The same work.** Every row hashes the keys and ages of what it found, in the order it found them, and a run in which two databases disagree fails rather than giving a time.
- **A clean start.** Each pass of each database runs in a process of its own, on new files, and the databases take turns in an order that changes every run. A time is the median of the runs.
- **The same machine.** The databases of one language run on one machine, one after another.

The cache each database may use is left at its default, apart from SQLite's, which is raised to DaruDB's 32 MiB. A database that keeps every object in memory, as Hive does, reads without touching its file.

The tables come from the `run-benchmarks` workflow, which runs each language on a GitHub-hosted Linux runner of its own. Those runners mount their disk without write barriers, so a sync commit there waits for the file system's work but not for the disk's own cache. They also differ in processor and disk from one run to the next, and the commit rows move most with them: through Node.js, a deferred commit of one object took SQLite 53 µs and DaruDB 25 µs on the EPYC 9V45 runner of one run, and 33 µs and 46 µs on the EPYC 7763 runner of the next, on the same day and the same code. The next section says why. Compare a commit row's databases within one table only.

## Next to Realm

For an application moving from Realm, the Node.js and Dart tables are the ones to read; choose Node.js or Dart in the sidebar to see them. DaruDB is ahead of Realm in every row but the first run of the query of one city, a field without an index, and in Dart the update as well. Realm keeps each property's values together and reads only the city, and takes 0.69 times DaruDB's time on the first run in Node.js and 0.73 times in Dart; over ten runs it takes 1.04 and 1.10 times, since from the second run on DaruDB reads the city where the first found it in each record. In Node.js and in Dart, Realm takes 2.2 and 3.2 times as long for a sync commit and has no deferred one, 6.4 and 3.4 times as long to insert 100,000 objects in one transaction, and 6.3 and 2.2 times to delete 10,000. It takes 3.8 and 2.5 times as long to read by key, and 13 and 7.3 times through the email, since it parses a query string on every call and has no unique index besides the primary key. It takes 4.6 and 3.8 times as long for the objects of one age, 112 and 98 times for the first 20 of five ages, since it sorts every object of the five ages before it takes the first 20, and 2.4 to 3.7 times for the ten objects with the highest score. An update sets one field in both, and takes Realm 1.11 times DaruDB's time in Node.js and 0.50 times in Dart.

## Where DaruDB is behind, and why

In this run Rust, Dart and Python ran on runners with AMD EPYC 9V45 processors, and Node.js on one with an AMD EPYC 7763. DaruDB is ahead of every other database in most rows in Node.js, Dart and Python, and in Python ahead of SQLite in every row but the first runs of the queries without an index. In Dart it is ahead in every row but the read by key and the queries without an index, where Hive, which keeps every object in memory, is ahead, and the update, where Realm is. In Rust, LMDB is ahead in the deferred commit, the reads and the queries but the count, and redb and SQLite in the queries through the age index and the first runs of those without one. These are the rows where DaruDB is not ahead, with what makes each slower:

- **A deferred commit of one object.** LMDB is ahead in every language it runs in, taking 0.36 to 0.51 times DaruDB's time, redb in Rust, taking 0.72 times, and SQLite in Node.js, taking 0.73 times. On Linux, about half of DaruDB's commit is spent in the kernel: it makes about fourteen system calls, ten writes for the eleven or so pages it writes and four to take and release the writer lock and to check for other writers and readers, since processes coordinate through file locks alone. LMDB makes seven writes and coordinates processes through a mutex in shared memory, which DaruDB rejects: a process that dies holding one can leave the others stuck. SQLite's deferred commits are not all of its work: it syncs its log and its file at each checkpoint of the log, every thousand pages, and on most runners its deferred commits spent two thirds of their time or more waiting for those syncs. The EPYC 7763 runners finish them sooner, and their processor, the oldest of the runner kinds, is the slowest at DaruDB's commit, which waits for nothing. In Rust, Dart and Python, on EPYC 9V45 runners, DaruDB's deferred commit took 0.42 to 0.44 times SQLite's time.
- **Sync commits** move from runner to runner more than any other row. In this run SQLite takes 0.85 times DaruDB's time in Node.js, on an EPYC 7763 runner, while in Rust, Dart and Python, on EPYC 9V45 runners, DaruDB is ahead of every other database. On the runners, a sync commit is mostly the file system's work. DaruDB writes a commit's pages where the free pages are, scattered over the file, where SQLite appends its pages to its log, and on the EPYC 7763 runners syncing twelve pages written scattered over a file took three times as long as syncing the same twelve written in one run, in a probe of the disk alone.
- **Reads and queries through an index, in Rust.** LMDB finds an object by its key in 0.68 times DaruDB's time and redb in 0.83 times, LMDB finds one by its email in 0.72 times, and LMDB, redb and SQLite run the queries through the age index in 0.48 to 0.82 times. A DaruDB lookup finds each page below the root in its page cache, under a lock and with a reference count, which takes a larger share of a lookup on these x86 processors than on Apple's, then searches the node comparing keys as bytes, and decodes the record, which tags each field with an id, into the struct. LMDB reads its file through a memory map, with no cache to look in, redb compares its primary keys as integers, and both read the record written by hand for them, with its fields at fixed places. DaruDB does not map its file, because another process can change mapped bytes under a reader, and a mapping does not mix with encryption. The sorted range costs more besides: it sorts by age descending and then by key, so DaruDB walks the age index backwards and holds back the objects of each age to give them in key order, where the loop written for the key-value stores asks for each age in turn. On an Apple M1 Max, DaruDB was ahead of LMDB in these rows but the sorted range.
- **A read by key in Dart**, where Hive takes 0.90 times DaruDB's time. Hive keeps every object of a box in memory and gives the one it holds, where DaruDB finds the record in its tree, through the page cache, and makes the object from it.
- **Queries without an index.** LMDB in Rust scans 2.6 to 7.1 times as fast, Hive in Dart 1.1 to 2.4 times, and Realm finds the objects of one city faster on the first run, in Node.js and in Dart. SQLite and redb are ahead on the first run alone: SQLite takes 0.78 to 0.96 times DaruDB's time in Rust, Node.js, Dart and Python, where it is level on the query of the highest scores, and redb 0.93 and 0.98 times in Rust. Over ten runs DaruDB is ahead of both, which take 1.20 to 1.54 times its time, and of Realm, which takes 1.04 to 3.7 times. The record written by hand for the key-value stores keeps the age and the score at fixed places, so a scan reads those bytes and decodes only what it returns; Realm keeps each property's values together, so a scan reads only the city; and SQLite's record lists the types of its columns at its start, so finding one column adds up the sizes before it without reading them. A DaruDB record tags each field with an id and a type, which makes renaming a field free and lets the engine check what it reads from a file it does not trust, so a first scan steps over the fields before the one it tests in each record. It remembers where it found the field, and the runs after the first read it there.
- **An update in Dart**, where Realm takes 0.50 times DaruDB's time, and 1.11 times in Node.js. Realm keeps each property's values together, so setting the age changes that value alone, where DaruDB writes the object's record again with the new age.
- **A lookup through a unique index in Node.js and Python**, where SQLite takes 0.89 times DaruDB's time in Node.js and LMDB 0.96 times in Python: two lookups make it, the email's entry in the index and then the object by its key, and making the object from its record in the language takes a share of it, a fifth in JavaScript.

## The complex suite

A second suite measures an application with more than one collection: three collections that link to each other, 401,000 objects in all, and queries through links, lists and several indexes at once. Only the stores with a query language run it, DaruDB, SQLite and, in Node.js and Dart, Realm: a key-value store has no planner, and the code written by hand for each query would be what a row measured, not the store.

These tables come from the same run as the ones above, each language on the runner it ran the first suite on, so Rust, Dart and Python ran on EPYC 9V45 runners and Node.js on an EPYC 7763. Each cell is the median of five passes, since a pass takes longer here.

<PerformanceTable suite="complex" />

### What it measures

| Collection | Objects | Fields |
| --- | --- | --- |
| Organizations | 1,000 | A code as the key, a name, a kind and a country, both indexed, and a year |
| People | 100,000 | A name, an email under a unique index, an age, a country and a date joined, each indexed, an embedded address, a score, whether active, a link to an organization, indexed, and a list of one to three tags, indexed |
| Posts | 300,000 | A link to the author, indexed, a title, a body of twelve words, likes, a language and a date, each indexed, whether pinned, and a list of up to two tags, indexed |

| Workload | What it does |
| --- | --- |
| Load 401,000 objects, 10,000 to a transaction | The three collections, each commit waiting for the disk; the time is per object |
| A person, their organization and their 5 newest posts | By key, then through the link, then through the posts' author index, 10,000 times |
| Three conditions on two indexes, sorted, first 50 | An age range of ten years, one country and active, by score descending |
| Count with OR across two indexes | The people of two countries, or aged 78 and over |
| Names that start with a text | The people whose name starts with `person` and a number below 1,000, a field without an index |
| A page of 50, deep in an indexed order | 50 people in the order they joined, after skipping up to 98,703 |
| Through a link | The 20 newest posts with 900 likes or more whose author lives in one country |
| Through two links | Counting the posts of one language whose author's organization is of one kind |
| A tag in a list, sorted by another index | The 20 posts with one tag that have the most likes |
| Count posts whose text contains a phrase | Two words in a row, in the body of 300,000 posts, without an index |
| Count with conditions on two indexes | The posts of one language with 500 likes or more |
| Update the people a query finds | Adding a year to the age of everyone in four countries, about 20,000 people, in one transaction; the time is per person |
| Delete 1,000 people with their posts | In one transaction; the time is per person |

Each store writes its queries the way an application would: DaruDB in its query language, prepared once; SQLite in SQL, with a join or a subquery for a link and a table of tags for each list; Realm in its query language, with a backlink for a person's posts, since Realm cannot index a link. An update through SQL is one `UPDATE` statement, where DaruDB and Realm find the people and change each. Every row digests what it found, and a run in which two stores disagree fails, as in the suite above.

### Where DaruDB is behind in the complex suite

DaruDB is ahead of SQLite in every row but the update, where SQLite takes 0.68 to 0.86 times DaruDB's time in Python, Node.js and Dart, and the two are level in Rust. Realm is ahead in four rows in Node.js and three in Dart:

- **Counting posts by a phrase in their text, and by conditions on two indexes.** Realm takes 0.44 and 0.68 times DaruDB's time in Node.js, and 0.53 and 0.69 times in Dart. Realm keeps each property's values together, so it reads only the bodies, or only the languages and the likes, and counts them without making an object. DaruDB reads each post's record to find the body in it, and the records of 300,000 posts outgrow its page cache of 32 MiB, so each count reads them from the file again; with a cache that held the file, the count took half the time on an Apple M1 Max. For the count by two conditions it gathers the keys of one condition from its index, then walks the other's index for those among them.
- **Updating the people a query finds.** Realm takes 0.66 times DaruDB's time in Node.js, and 1.14 times in Dart, and SQLite 0.68 to 0.86 times outside Rust. Realm sets one value where it keeps the ages, and SQLite's one statement changes every row inside the engine. DaruDB finds the people through the country index and changes each one's record where it lies, with its entries in the age index, 6.3 to 9.5 µs a person.
- **Posts through a link, in Dart.** Realm takes 0.94 times DaruDB's time in Dart, and 1.61 times in Node.js, where making a JavaScript object of each post it finds costs more. Realm tests the likes and each author's country in the values it keeps together. DaruDB gathers the keys of the country's people from its index, follows them to their posts through the author index, meets those with the likes, and walks the date index for the newest.
- **Deleting people with their posts, in Node.js.** Realm takes 0.84 times DaruDB's time in Node.js, on an EPYC 7763 runner, and 1.46 times in Dart, on an EPYC 9V45. DaruDB deletes each post's record with its entries in five indexes, and its commit syncs the pages it changed, scattered over the file, which the EPYC 7763 runners sync more slowly, as the sync commits above show.

## Run it yourself

The harness is in [`bench/`](https://github.com/jooy2/darudb/tree/main/bench) of the repository, with a README that lists every setting. The `run-benchmarks` workflow runs it on GitHub-hosted runners and opens a pull request with the results for this page.

```bash
cd bench/rust && cargo build --release && cd ../..
node bench/run.mjs rust --runs 9 --out bench/results/rust.json
```

`--suite complex` runs the complex suite: `node bench/run.mjs rust --suite complex --runs 9 --out bench/results/rust-complex.json`.
