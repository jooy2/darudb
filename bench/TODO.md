# Performance follow-ups

Work that could close the gaps the benchmark shows, with what has been measured so far, so that a later session can pick an item up cold. Each item says where the time goes, what was tried, and how to tell whether a change helps. `CLAUDE.md`, under "Where the engine is behind, and why", has the short form of the causes; the [performance page](../docs/en/performance.md) has the results.

This file names other databases, as everything in `bench/` may. Commit messages for the work it describes may not.

## How to measure

Measure on a GitHub-hosted Linux runner, where the published numbers come from, and on Apple silicon too when a change touches code both run. A change is worth keeping when the two sides' ranges do not overlap over at least 15 rounds on one runner; changes within about 1.5% have not been kept before.

The branch `investigate/linux-perf` holds the probes. It is never merged. Rebase it onto `main` before using it (`git rebase main`, then `git push --force-with-lease`). Its workflows each run on a push that touches their own files:

| Workflow | Runs on a push to | What it does |
| --- | --- | --- |
| `probe-linux.yml` | `bench/probe/variants/*.patch` | Builds the Rust harness for the branch as it is and once more with each patch applied, then runs `bench/probe/ab.mjs`: 15 rounds of DaruDB passes, the sides taking turns, with each row's median, range and ratio |
| `probe-machines.yml` | `bench/probe/machines.sh`, `bench/probe/syscost.c` | The same probe on ten runners at once, which land on different kinds of machine: the machine, the cost of single system calls (`syscost.c`), three passes of each Rust store, and `perf stat` and `perf record` of each store's deferred commits |
| `probe-profile.yml` | `bench/probe/profile.sh` | `perf record` with call graphs of single rows, DaruDB's beside LMDB's, with own time, time with callees, and callers |

The branch's Rust harness takes three environment variables: `BENCH_FOCUS=<row>` runs that row `BENCH_REPEAT` times over and ends the pass after it, so that a profile is mostly that row; `BENCH_SKIP=<row>,<row>` leaves rows out; and every row starts with a `stat` of `/bench-marker/<row>`, which a trace shows, so that `bench/probe/syscalls.py` can count the system calls of each row in an `strace -f -T` log.

`syscost.c` syncs the file before it times `fdatasync` and `fsync`, so that neither counts the flush of what its other loops wrote; a run before 2026-10-11 did not, and its `fdatasync` line is not to be trusted.

The A/B tools for Apple silicon are kept outside the repository, since they build two engines into one binary from commits unpacked elsewhere.

## The runners

GitHub has handed out four kinds of machine for `ubuntu-24.04` so far, and which a job lands on changes the commit rows most. Measured on 2026-10-10 with `probe-machines.yml` (one-object deferred commits, Rust harness):

| Runner | DaruDB | SQLite | SQLite waiting | `pwrite` 4 KiB | `F_SETLK` pair | `F_GETLK` |
| --- | --- | --- | --- | --- | --- | --- |
| AMD EPYC 7763, `sda` | 36 µs | 30 µs | 23% | 1.03 µs | 1.64 µs | 0.46 µs |
| AMD EPYC 9V45, `nvme` | 22 µs | 50 µs | 65–70% | 0.69 µs | 1.15 µs | 0.30 µs |
| AMD EPYC 9V74, `nvme` | 30 µs | 66–105 µs | 67–79% | 0.92 µs | 1.57 µs | 0.52 µs |
| Intel Xeon 8573C, `sda` | 25 µs | 56 µs | 74% | 0.49 µs | 0.63 µs | 0.22 µs |

DaruDB's and LMDB's deferred commits wait for nothing (0.99 of a processor), so they follow the processor; the 7763 is the slowest. SQLite's `synchronous = NORMAL` commits sync the log and the file at each checkpoint, every 1,000 log pages, and spend most of their time waiting for that, except on the 7763 runners, whose disk finishes it sooner. Every runner mounts `/` with `nobarrier`, so no sync reaches the disk's cache there.

## Items

### 1. Reads and queries through an index in Rust on x86

On every x86 runner so far, LMDB takes 0.50 to 0.74 times DaruDB's time in `get-key`, `get-email`, `age-equal` and `age-range`, and redb 0.58 to 0.88 times in all of them but `get-email`, where the two are level; on an Apple M1 Max, DaruDB was ahead of LMDB in all of them but `age-range`. A profile of `get-key` repeated 30 times on an EPYC 7763 (`probe-profile.yml`):

| Function | Own time | What it is |
| --- | --- | --- |
| `NodeRef::rank` and its closure | 29% | The search inside a node, comparing keys as bytes after their four-byte heads |
| `schema::typed::read_record` | 17% | Decoding the record into the struct, field id by field id |
| `Loader::load` | 17% | The page cache: the hash table probe 6%, the `Arc` clone 3.4%, the mutex the rest |
| `malloc`, `free`, `from_utf8` | 6% | The struct's strings, which the other stores' structs allocate too |

LMDB spends 52% in its node search and reads mapped memory with no cache. redb 4.3 does not map its file: it reads through a striped `RwLock` cache of `Arc<[u8]>` pages, as DaruDB does, compares its `u64` keys as integers, and reads the record written by hand, with fixed places.

Tried and not kept:

- `parking_lot::Mutex` for the page cache: within ±1% on Apple silicon, within ±1.4% on every row on an EPYC 9V45, and on an EPYC 7763 −5% on `get-key` and +5% on `city-scan`, with the ranges overlapping. No lock type removes the atomic operations of a lookup.
- A per-thread memo of 64 branch nodes in front of the page cache (`memo.patch`, in the branch's history): `get-key` −5.4%, but one-object deferred commits +14%, because a writer cannot take a branch the memo holds and copies it, and `age-equal` +3.8%, since storing into the memo on a miss costs atomics too.

Directions not tried yet:

- **Keep the path in the read transaction.** A read transaction could hold the root and the branches its last lookup passed, as `btree::read::Seeker` does for a query, for the lookups by key that come one after another. It ends with the transaction, so it would not keep a writer from taking a node afterwards, which is what sank the memo. Measure `get-key`, `get-email` and one-object deferred commits together.
- **Integer primary keys.** An auto-increment key is encoded as bytes and compared as bytes. A comparison specialised for the encoding of integers, or heads taken from the integer, could shorten `rank` for primary-key trees. Check `format/object/key.rs` and the heads in `btree/node.rs`.
- **Decoding in layout order.** `read_record` walks the record and the layout together by field id. Records written by the same schema hold their fields in the layout's order, so a fast path that checks the order once per record and copies field by field could skip most of the matching. Check `schema/typed.rs`.
- **Readers without reference counts.** The structural change: readers borrow nodes for the life of the transaction, reclaimed through epochs or hazard pointers, so that a lookup takes no lock and touches no count. Large, and it touches every read path; do it only if the items above leave the gap.

### 2. Updates in Rust

LMDB took 0.77 times DaruDB's time in `update` on an EPYC 7763, while DaruDB was 12% ahead on an EPYC 9V45, both with the engine as it is now. Nobody has profiled it. The row reads 10,000 random objects, `put`s each back with a new age, and commits. Profile it with `probe-profile.yml` (`daru:update:1 lmdb:update:1`; `update` is a row timed whole, so `BENCH_REPEAT` does not repeat it) to tell whether the commit, whose pages go where earlier commits freed pages, or the `put`s take the time.

### 3. Small commits write scattered pages

A one-object deferred commit makes about ten write calls for eleven pages, against LMDB's seven, and a sync commit writes them scattered, where SQLite appends to its log. That is most of DaruDB's kernel time in a commit, about half of the commit on Linux. The commit writes its pages where the free pages are; two prototypes that grew the file to place them in runs made it a third larger and slowed reads and writes by 7 to 27% once it outgrew the page cache (`CLAUDE.md`, "A small commit writes its pages where the free pages are").

Directions not tried yet:

- **One system call for all the writes on Linux.** `io_uring` can submit every write of a commit at once. It needs a dependency or raw system calls, works only on Linux 5.1 and later, is disabled in some containers, and needs a fallback; measure how much of the ten calls' cost it saves before weighing that.
- **A bounded area for small commits.** Keep a few runs of free pages near the end of the file for small commits, grown by a fixed amount, so that their pages land in runs without the file growing as the prototypes did.

### 4. The other system calls of a commit

Besides the writes, a commit takes and releases the writer lock (one `F_SETLK` each), tests the turn lock (`F_GETLK`) and tests the snapshot bytes (`F_GETLK`). The test of the file's length is gone (`98be56b`).

- **The turn lock test costs about 1%.** Removing it outright (`noturn.patch`, in the branch's history) made a deferred commit on an EPYC 7763 1.4% faster, with overlapping ranges. Testing it once a millisecond would keep the turn's bound and save less. A single call that takes the writer lock and fails while the turn is claimed would need the writer byte moved, since the bytes beside it are the open lock and the recovery lock, and moving it breaks running beside older versions. Left as it is.
- **The snapshot test** (`Locks::reclaimable` and `young_reclaimable` in `lock/mod.rs`) runs at every commit that has groups to reclaim. Not measured on its own yet; a variant that skips it when nothing could be reclaimed would say what it costs.

### 5. Queries without an index

LMDB scans several times faster in Rust, redb about twice as fast in Rust on an EPYC 7763 but level on a 9V45, Hive up to two and a half times in Dart, and SQLite 6 to 15% faster in Node.js, Dart and Python. A DaruDB record tags each field with an id and a type, so a scan steps over the fields before the one it tests; the stores ahead read fixed places, or, as SQLite, add up the sizes the record's header lists. Changing the record is a new format version, with a migration (`CLAUDE.md`, "The file format is released"). Short of that:

- **Field offsets per leaf.** A scan could work out where the tested field lies in each record once and keep it with the cached leaf, as the leaf keeps the heads of its keys.
- **Profile `city-scan` and `top-score` on x86 first**: the share of stepping over fields against reading leaves through the page cache has only been measured on Apple silicon (two fifths of such a scan).

### 6. A lookup through a unique index in Node.js

SQLite takes 0.81 to 0.89 times DaruDB's time in `get-email` through the Node.js package. The cause recorded is that the first lookup to reach a page of the index loads it, checks it and works out the heads of its keys; with 20,000 lookups over the index, that should fade after the first few hundred. Profile the row with `node --cpu-prof` and the addon's symbols to see whether it is the cold pages or the per-call cost of the package.

### 7. The benchmark itself

- **The runner decides more than the code.** Which store leads the commit rows, and even the scans in Rust, changes with the kind of runner. Options: run all four languages in one job, so that the tables at least share a machine; or run each language on several runners and show the kind with each table, as `probe-machines.yml` does.
- **Rows that spread.** A Dart sync commit spread 599% between passes in one run. More passes, or fewer commits per pass with more passes, would steady the commit rows.
- **Realm in Dart.** Realm is measured through Node.js only. Its Dart package downloads its library with `dart run realm install` and is built for Flutter; adding it would need that step in the workflow.
- **`update` in Rust and Dart.** The Node.js and Python passes set the field with `update`; Rust and Dart write the typed object back with `put`, which costs little there. Measuring `update` there too would show whether the engine's update path is as fast.
