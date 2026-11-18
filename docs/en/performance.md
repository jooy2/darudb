---
title: Performance
order: 3
aside: false
pageClass: performance-page
---

# Performance

How fast DaruDB reads and writes next to the embedded databases each of its languages uses most, measured with the benchmark in the repository: the same objects and workloads in every database, at the same durability settings, on one machine.

The table follows the language chosen in the sidebar. The stores of one language ran one after another, so their times compare with each other. Every language ran on the same machine, each in a run of its own.

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

The cache each database may use is left at its default, apart from DaruDB's and SQLite's, which are both set to 32 MiB. A database that keeps every object in memory, as Hive does, reads without touching its file.

The tables were measured on one Apple M1 Max with 10 cores and 32 GiB of memory, under macOS 27.2, with the files on its internal SSD. On Apple systems a sync commit of DaruDB, LMDB and redb, and of SQLite with `fullfsync`, ends with `F_FULLFSYNC`, which waits until the drive has written its own cache. DaruDB's sync commit takes 4.1 to 4.9 ms here, nearly all of it that wait, since the same commit deferred takes 35 to 48 µs. Hive's `flush` is an `fsync`, which on Apple systems reaches only the drive's cache, and Realm's commit is an `F_BARRIERFSYNC`, which keeps writes in order across a power cut but does not wait for the drive to have them. Their sync commits take a fraction of the others' time here and promise less, since a power cut can undo them.

The `run-benchmarks` workflow measures on GitHub-hosted Linux runners instead. Those runners mount their disk without write barriers, so a sync commit there waits for the file system's work but not for the disk's own cache, and their processors and disks differ from one run to the next, which moves the commit rows most. Compare a commit row's databases within one table only.

## Next to Realm

For an application moving from Realm, the Node.js and Dart tables are the ones to read; choose Node.js or Dart in the sidebar to see them. DaruDB is ahead of Realm in every row but the sync commit, the first run of the query of one city, a field without an index, and the update. Realm's sync commit takes 0.17 times DaruDB's time in Node.js and 0.13 times in Dart, since on Apple systems it does not wait for the drive to have the commit, as the section above says, and it has no deferred commit. Realm keeps each property's values together and reads only the city, and takes 0.64 times DaruDB's time on the first run of that query in Node.js and 0.75 times in Dart; over ten runs it takes 1.39 and 1.12 times, since from the second run on DaruDB reads the city where the first found it in each record. An update sets one field in both, and takes Realm 1.04 times DaruDB's time in Node.js and 0.68 times in Dart.

In the other rows, in Node.js and in Dart, Realm takes 7.8 and 4.2 times as long to insert 100,000 objects in one transaction, and 5.2 and 3.5 times to delete 10,000. It takes 4.9 and 2.9 times as long to read by key, and 11 and 6.6 times through the email, since it parses a query string on every call and has no unique index besides the primary key. It takes 4.1 and 4.0 times as long for the objects of one age, 118 and 85 times for the first 20 of five ages, since it sorts every object of the five ages before it takes the first 20, 7.4 and 8.5 times to count through the index, and 1.9 to 3.8 times for the ten objects with the highest score.

## Where DaruDB is behind, and why

On this machine DaruDB is ahead of every other database in most rows in every language. In Python it is ahead of SQLite in every row but the sync commit, and of LMDB in every row but the deferred commit. In Node.js, LMDB is ahead in the deferred commit, SQLite in the first run of the query of the highest scores, and Realm in the rows the section above names, while SQLite is level with DaruDB in the sync commit, the read through the email, the sorted range and the first run of the query of one city. In Dart, Hive is ahead in the queries without an index, SQLite in the first run of the query of the highest scores, and Realm in the rows above. In Rust, LMDB is ahead in the deferred commit, the sorted range and the queries without an index, and redb in the deferred commit, the read by key, the sorted range and the first run of the query of the highest scores. These are the rows where DaruDB is not ahead, with what makes each slower:

- **A sync commit.** Every database that waits for the drive takes 4.1 to 4.9 ms for one here, nearly all of it the wait, and LMDB through Python takes 8.9 ms. In Rust, Node.js and Dart they are within a twentieth of each other. In Python SQLite takes 0.90 times DaruDB's time, where DaruDB's runs spread wider, from 4.2 to 8.1 ms. Hive and Realm take 0.03 to 0.17 times DaruDB's time, since on Apple systems they do not wait for the drive, as the section on fairness says.
- **A deferred commit of one object.** LMDB takes 0.61 to 0.82 times DaruDB's time in Rust, Node.js and Python, and redb 0.68 times in Rust. DaruDB writes about eleven pages in nine or ten calls, each page with its check, and it takes the writer lock with `fcntl`, reads the header again and releases the lock, since processes coordinate through file locks alone; on an Apple M1 Max the lock and the header are about an eighth of the commit. LMDB writes its pages with no check and coordinates processes through a mutex in shared memory, which DaruDB rejects: a process that dies holding one can leave the others stuck. redb's `Durability::None` makes nothing durable until a later immediate commit, so it promises less than the others' deferred commits. SQLite and Hive are behind here, at 1.63 to 2.00 and 1.16 times DaruDB's time.
- **Reads and queries through an index, in Rust.** redb finds an object by its key in 0.88 times DaruDB's time, and redb and LMDB give the first 20 objects of five ages, sorted, in 0.71 and 0.81 times. A DaruDB lookup finds each page below the root in its page cache, searches the node comparing keys as bytes, and decodes the record, which tags each field with an id, into the struct. redb compares its primary keys as integers, and reads the record written by hand for it, with its fields at fixed places. The sorted range costs more besides: it sorts by age descending and then by key, so DaruDB walks the age index backwards and holds back the objects of each age to give them in key order, where the loop written for the key-value stores asks for each age in turn. In the other reads and queries through an index DaruDB is ahead of both, and LMDB, which reads its file through a memory map, takes 1.06 and 1.14 times its time to read by key and by email. On GitHub's x86 Linux runners, where finding a page in the cache under its lock takes a larger share of a lookup, LMDB was ahead in those rows too.
- **Queries without an index.** LMDB in Rust scans 2.3 to 6.3 times as fast as DaruDB, Hive in Dart 1.2 to 2.8 times, and Realm finds the objects of one city faster on the first run, in Node.js and in Dart. SQLite is ahead on the first run of the query of the highest scores in Node.js and in Dart, at 0.91 and 0.82 times DaruDB's time, and level on the first runs in Rust, and redb is ahead on that first run in Rust, at 0.84 times. Over ten runs DaruDB is ahead of SQLite, redb and Realm, which take 1.12 to 3.8 times its time. The record written by hand for the key-value stores keeps the age and the score at fixed places, so a scan reads those bytes and decodes only what it returns; Hive scans the objects it keeps in memory; Realm keeps each property's values together, so a scan reads only the city; and SQLite's record lists the types of its columns at its start, so finding one column adds up the sizes before it without reading them. A DaruDB record tags each field with an id and a type, which makes renaming a field free and lets the engine check what it reads from a file it does not trust, so a first scan steps over the fields before the one it tests in each record. It remembers where it found the field, and the runs after the first read it there.
- **An update in Dart**, where Realm takes 0.68 times DaruDB's time, and 1.04 times in Node.js. Realm keeps each property's values together, so setting the age changes that value alone, where DaruDB writes the object's record again with the new age.

## The complex suite

A second suite measures an application with more than one collection: three collections that link to each other, 401,000 objects in all, and queries through links, lists and several indexes at once. Only the stores with a query language run it, DaruDB, SQLite and, in Node.js and Dart, Realm: a key-value store has no planner, and the code written by hand for each query would be what a row measured, not the store.

These tables come from the same machine as the ones above, each language's measured right after its first suite. Each cell is the median of five passes, since a pass takes longer here.

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

DaruDB is ahead of SQLite in every row but the update in Dart and Python, where SQLite takes 0.64 and 0.91 times DaruDB's time; in Rust and Node.js it takes 1.38 and 1.26 times. Realm is ahead in three rows in Node.js and five in Dart:

- **Counting posts by a phrase in their text, and by conditions on two indexes.** Realm takes 0.45 and 0.82 times DaruDB's time in Node.js, and 0.45 and 0.71 times in Dart. Realm keeps each property's values together, so it reads only the bodies, or only the languages and the likes, and counts them without making an object. DaruDB reads each post's record to find the body in it, and the records of 300,000 posts outgrow the page cache of 32 MiB it has here, so each count reads them from the file again; with a cache that held the file, the count took half the time on an Apple M1 Max. For the count by two conditions it gathers the keys of one condition from its index, then walks the other's index for those among them.
- **Updating the people a query finds.** Realm takes 0.66 times DaruDB's time in Node.js and 0.57 times in Dart, and SQLite 0.64 times in Dart and 0.91 times in Python. Realm sets one value where it keeps the ages, and SQLite's one statement changes every row inside the engine. DaruDB finds the people through the country index and changes each one's record where it lies, with its entries in the age index, 5.1 to 8.3 µs a person.
- **Names that start with a text, and posts through a link, in Dart.** Realm takes 0.74 and 0.93 times DaruDB's time in Dart, and 1.04 and 1.76 times in Node.js. Realm tests the names, or the likes and each author's country, in the values it keeps together. DaruDB walks every person's record for the names, a field without an index, and for the posts gathers the keys of the country's people from its index, follows them to their posts through the author index, meets those with the likes, and walks the date index for the newest.

## Run it yourself

The harness is in [`bench/`](https://github.com/jooy2/darudb/tree/main/bench) of the repository, with a README that lists every setting. The `run-benchmarks` workflow runs it on GitHub-hosted runners and opens a pull request with the results for this page.

```bash
cd bench/rust && cargo build --release && cd ../..
node bench/run.mjs rust --runs 9 --out bench/results/rust.json
```

`--suite complex` runs the complex suite, for which five passes are enough: `node bench/run.mjs rust --suite complex --runs 5 --out bench/results/rust-complex.json`.
