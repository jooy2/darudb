# Benchmark

This folder measures DaruDB against the embedded databases applications reach for most in each language it ships for: the same objects, the same workloads and the same durability in every store, so that the times compare. The documentation's [performance page](https://darudb.cdget.com/performance) shows the results, and `.github/workflows/run-benchmarks.yml` runs it on GitHub-hosted runners. [TODO.md](TODO.md) lists the work that could close the gaps it shows, and how to measure it.

It is the one place in the repository besides the documentation's comparison, performance and migration pages that names other databases, since a comparison cannot be made without them.

## What runs

| Language | DaruDB | Compared with |
| --- | --- | --- |
| Rust | The crate, typed by derive | SQLite (rusqlite, bundled), LMDB (heed), redb |
| Node.js | `packages/node` | SQLite (better-sqlite3), LMDB (lmdb-js), Realm (realm-js) |
| Dart | `packages/dart/darudb` | SQLite (the `sqlite3` package), Hive CE, Realm (realm_dart) |
| Python | `packages/python` | SQLite (the standard library's `sqlite3`), LMDB (py-lmdb) |

Each store is used the way an application in that language would use it: DaruDB through its public API with its schema, SQLite through SQL with prepared statements, and the key-value stores with a record and indexes written by hand. Every read is made into an object of the language, with all its fields. An update that changes one field sets that field where the language's package can, as SQL sets one column: DaruDB's Node.js, Python and Dart passes use `update`, where writing the whole object back would build it again in the language first, and the Rust pass writes the typed struct back with `put`, which is faster there than an `update`, whose changes are a dynamic `Object`.

Realm is there for applications moving from it, in Node.js and in Dart, though its vendor deprecated it in September 2024 and ended support in September 2025. In Dart its pass is a harness of its own, `bench/dart_pinned`, since its code generator pins an older analyzer than DaruDB's and one package cannot resolve both; it reads the workloads from `bench/dart/lib/common.dart` by path, and `bench/run.mjs` runs both harnesses as one. It assigns no keys, so its pass gives each object the key the other stores assign; it has no unique index besides the primary key, so `email` has a plain index; its queries are Realm Query Language strings, which it parses on every call, since it has no prepared queries; and an update sets the one property.

## The data

One collection, `people`, of 100,000 objects keyed by an auto-increment, where `n` counts from 0 and the object gets the key `n + 1`:

| Field   | Value              | Index  |
| ------- | ------------------ | ------ |
| `name`  | `person <n>`       | None   |
| `email` | `<n>@example.com`  | Unique |
| `age`   | `n * 7919 mod 80`  | Index  |
| `city`  | `city <n mod 100>` | None   |
| `score` | `n * 0.618 mod 1`  | None   |

Random reads go in the order a 64-bit mix of the read's number gives, the same in every language.

## The workloads

| Row | What it measures | Operations |
| --- | --- | --- |
| `insert-sync` | Inserting one object per commit, each commit waiting for the disk | 500 |
| `insert-deferred` | Inserting one object per commit, each commit leaving the sync for later | 10,000 |
| `insert-bulk` | Inserting 100,000 objects in one transaction, the commit included | 100,000 |
| `get-key` | Reading one object by its primary key, in random order | 100,000 |
| `get-email` | Reading one object through the unique index on `email`, in random order | 20,000 |
| `age-equal` | Finding the 1,250 objects of one `age`, through its index | 200 |
| `age-range` | Finding the first 20 objects of a range of five ages, sorted by `age` descending, then by key | 5,000 |
| `count` | Counting the objects with `age >= 40`, through the index | 200 |
| `city-scan` | Finding the 1,000 objects of one `city`, which has no index | 10 |
| `city-scan-first` | The first of the ten runs of `city-scan` alone | 1 |
| `top-score` | Finding the 10 objects with the highest `score`, which has no index | 10 |
| `top-score-first` | The first of the ten runs of `top-score` alone | 1 |
| `update` | Reading 10,000 objects and writing each back with a new `age`, in one transaction, the commit included | 10,000 |
| `delete` | Deleting 10,000 objects in one transaction, the commit included | 10,000 |

Reads run in one read transaction. A query that runs many times is prepared once, as a statement would be. A row's time is the time of all its operations divided by their number. The first run of each query without an index is timed as a row of its own as well (`FIRST_RUN` in each harness's common code), since a store may answer such a query faster once it has run it; the digest of the row of ten then covers the nine runs after the first.

## The complex suite

`--suite complex` runs a second suite, closer to an application with more than one collection: three collections that link to each other, ten times the objects of the load, and queries through the links, lists and several indexes at once. Only the stores with a query language run it, DaruDB, SQLite and, in Node.js and Dart, Realm: a key-value store has no planner, and the code written by hand for each of these queries would be what the row measured, not the store.

| Collection | Objects | Key | Fields |
| --- | --- | --- | --- |
| `organizations` | 1,000 | `code`, a string | `name`, `kind` (index), `country` (index), `founded` |
| `people` | 100,000 | an auto-increment | `name`, `email` (unique), `age` (index), `country` (index), `address`, an embedded object of `city` and `zip`, `score`, `active`, `joinedAt` (index), `organization`, a link (index), `tags`, a list of one to three strings (index) |
| `posts` | 300,000 | an auto-increment | `author`, a link to a person (index), `title`, `body` of twelve words, `likes` (index), `language` (index), `createdAt` (index), `pinned`, `tags`, a list of up to two strings (index) |

Every field is drawn from a 32-bit hash of the object's number and a salt of its own, the same in every language. SQLite keeps what an SQL application would: a foreign key for each link, the address's fields as columns, and a table of `(post, tag)` and one of `(person, tag)` for the lists, with an index on every column the other stores index; a row is read with its tags, through a subquery. Realm cannot index a link, so a person's posts are a backlink there, and it does not index a list.

| Row | What it measures | Runs |
| --- | --- | --- |
| `load` | The three collections, 10,000 objects to a transaction, each commit waiting for the disk; the time is per object | 401,000 objects |
| `person-detail` | A person by key, their organization through the link, and their five newest posts | 10,000 |
| `people-filter` | `age` in a range of ten, `country` equal and `active`, sorted by `score` descending, first 50 | 500 |
| `people-or` | Counting the people of two countries or aged 78 and over | 100 |
| `people-prefix` | The people whose name starts with `person` and a number below 1,000 | 100 |
| `people-page` | 50 people in `joinedAt` order, after skipping up to 98,703 | 100 |
| `posts-author-country` | Posts with 900 likes or more whose author lives in one country, newest first, first 20 | 100 |
| `posts-two-links` | Counting the posts of one language whose author's organization is of one kind | 30 |
| `posts-tag` | Posts with one tag, by likes descending, first 20 | 200 |
| `posts-text` | Counting the posts whose body contains two words in a row | 10 |
| `posts-count` | Counting the posts of one language with 500 likes or more | 200 |
| `update-query` | One transaction that adds a year to the age of everyone in four countries; the time is per person | about 20,000 |
| `delete-cascade` | One transaction that deletes 1,000 people and every post each wrote; the time is per person | 1,000 |

The objects of the load are made before its time starts. Each store writes its query the way an application would: DaruDB in its query language, prepared once; SQLite in SQL, with a join or a subquery for a link; Realm in its query language, with a slice of the sorted results for the page, since it has no offset. An update through SQL is one `UPDATE` statement, where DaruDB and Realm find the people and change each. Results are digested as in the simple suite, the people by key and age and the posts by key and likes, and the rows `left-people` and `left-posts` check what is left at the end.

## Durability

A sync commit waits until the disk has the commit; a deferred one returns before that and reaches the disk with a later sync. Each store is set to promise the same:

| Store | Sync commit | Deferred commit |
| --- | --- | --- |
| DaruDB | `commit`, the default | `commit_deferred`, `durability: 'deferred'` and their equivalents |
| SQLite | WAL with `synchronous = FULL` | WAL with `synchronous = NORMAL` |
| LMDB | The default commit | `NO_SYNC`, synced once at the end |
| redb | `Durability::Immediate` | `Durability::None`, which makes nothing durable until a later immediate commit |
| Hive CE | A write and `flush` | A write without `flush`, flushed once at the end |
| Realm | The default commit | None: every commit syncs, so the row has no Realm time |

SQLite runs with `fullfsync = ON`, so that on Apple systems it flushes as DaruDB and LMDB do there; it changes nothing elsewhere. Hive's `flush` is an `fsync`, which on Apple systems does not reach the disk's own cache, and Realm's commit there is an `F_BARRIERFSYNC`, which keeps writes in order across a power cut but does not wait for the disk to have them, so the sync commits of both promise less there than the others'. On Linux, where the workflow measures, every sync commit is an `fsync` or an `fdatasync`.

## How the runs are put together

- **A process for every pass.** Each pass of each store runs in a process of its own, on new files in a temporary directory, so that no store inherits another's page cache, allocator or open files.
- **Turns.** The stores take turns within a run, and the order moves by one each run, so that no store always runs first or last.
- **The median.** A cell is the median of the runs, with the fastest and the slowest kept beside it.
- **Checked results.** Every row hashes the keys and ages of what it found, in the order it found them. The stores of a language have to agree on every row, and every run of a store on itself; otherwise the run fails, because stores that did different work do not compare. A store with no way to do a row, such as a deferred commit in a store whose every commit syncs, reports a time of `null` for it, and the page leaves that cell empty.
- **One machine.** The stores of one language run on one machine, one after another, so their times compare with each other. Times of different languages come from different runs and machines, and do not.

## Running it

From the repository root, after building what each language needs:

```bash
cd bench/rust && cargo build --release && cd ../..
node bench/run.mjs rust --runs 9 --out bench/results/rust.json
```

```bash
(cd packages/node && npm ci && npm run build) && (cd bench/node && REALM_DISABLE_ANALYTICS=1 npm ci)
node bench/run.mjs node --runs 9 --out bench/results/node.json
```

`REALM_DISABLE_ANALYTICS` keeps Realm's postinstall script from sending usage data. Its install script downloads its library, which npm 12 and later skip, since they run no install scripts unless allowed; then download it by hand:

```bash
(cd bench/node/node_modules/realm && ../.bin/prebuild-install --runtime napi)
```

```bash
(cd bench/dart && dart pub get && dart run build_runner build && dart build cli)
(cd bench/dart_pinned && export REALM_DISABLE_ANALYTICS=1 && dart pub get && dart run build_runner build && dart run realm_dart install && dart build cli && dart run tool/bundle.dart)
node bench/run.mjs dart --runs 9 --out bench/results/dart.json
```

`dart run realm_dart install` downloads Realm's library for this system into the package's folder, and `tool/bundle.dart` copies it beside the compiled harness, where a compiled program looks for it. `REALM_DISABLE_ANALYTICS` keeps Realm's tools from sending usage data.

```bash
python3 -m venv bench/python/.venv && bench/python/.venv/bin/pip install maturin -r bench/python/requirements.txt
(cd packages/python && VIRTUAL_ENV=../../bench/python/.venv PATH=../../bench/python/.venv/bin:$PATH maturin develop --release)
BENCH_PYTHON=bench/python/.venv/bin/python node bench/run.mjs python --runs 9 --out bench/results/python.json
```

`--suite complex` runs the complex suite, for which five passes are enough, since each takes longer: `node bench/run.mjs rust --suite complex --runs 5 --out bench/results/rust-complex.json`. `--only daru,sqlite` limits the stores, and `--dir` chooses where the files go. `node bench/merge.mjs bench/results/*.json` writes the results into `docs/.vitepress/theme/performance.json`, which the documentation reads, each suite's apart.
