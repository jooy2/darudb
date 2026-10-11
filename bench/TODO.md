# Performance follow-ups

Work that could close the gaps the benchmark shows, with what has been measured so far, so that a later session can pick an item up cold. Each item says where the time goes, what was tried, and how to tell whether a change helps. `CLAUDE.md`, under "Where the engine is behind, and why", has the short form of the causes; the [performance page](../docs/en/performance.md) has the results.

This file names other databases, as everything in `bench/` may. Commit messages for the work it describes may not.

## How to measure

Measure on a GitHub-hosted Linux runner, where the published numbers come from, and on Apple silicon too when a change touches code both run. A change is worth keeping when the two sides' ranges do not overlap over at least 15 rounds on one runner; changes within about 1.5% have not been kept before. Two builds of the same engine, one with a comment changed, moved the read rows up to 4% apart in one run on an EPYC 7763 (2026-10-11), and a change that did not touch the scans moved the sort of the top 10 by 15% once, through where the code landed: a change within that, on rows it does not touch, is not a result. Probe a change beside the base twice before believing a row it should not move.

The branch `investigate/linux-perf` holds the probes. It is never merged. Rebase it onto `main` before using it (`git rebase main`, then `git push --force-with-lease`). Its workflows each run on a push that touches their own files:

| Workflow | Runs on a push to | What it does |
| --- | --- | --- |
| `probe-linux.yml` | `bench/probe/variants/*.patch` | Builds the Rust harness for the branch as it is and once more with each patch applied, then runs `bench/probe/ab.mjs`: 15 rounds of DaruDB passes, the sides taking turns, with each row's median, range and ratio |
| `probe-machines.yml` | `bench/probe/machines.sh`, `bench/probe/syscost.c` | The same probe on ten runners at once, which land on different kinds of machine: the machine, the cost of single system calls (`syscost.c`), three passes of each Rust store, and `perf stat` and `perf record` of each store's deferred commits |
| `probe-profile.yml` | `bench/probe/profile.sh` | `perf record` with call graphs of single rows, DaruDB's beside LMDB's, with own time, the time of each system call and of the engine's functions with their callees, and callers. Every row before the focused one but the bulk insert is left out, and a row that inserts is sampled at a higher rate instead of repeated, since its keys would be taken |
| `probe-uring.yml` | `bench/probe/uring.c` | Ten scattered 4 KiB writes as ten `pwrite` calls against one io_uring submission, on three runners |

The branch's Rust harness takes three environment variables: `BENCH_FOCUS=<row>` runs that row `BENCH_REPEAT` times over and ends the pass after it, so that a profile is mostly that row; `BENCH_SKIP=<row>,<row>` leaves rows out; and every row starts with a `stat` of `/bench-marker/<row>`, which a trace shows, so that `bench/probe/syscalls.py` can count the system calls of each row in an `strace -f -T` log.

`syscost.c` syncs the file before it times `fdatasync` and `fsync`, so that neither counts the flush of what its other loops wrote; a run before 2026-10-11 did not, and its `fdatasync` line is not to be trusted.

The A/B tools for Apple silicon are kept outside the repository, since they build two engines into one binary from commits unpacked elsewhere. On a Mac busy with other work, wall time spreads 30% and more between passes; time the harness's rows by the thread's CPU time on a performance core instead, which a local copy of `work.rs` can do with `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` and `pthread_set_qos_class_self_np`.

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

On every x86 runner so far, LMDB takes 0.50 to 0.74 times DaruDB's time in `get-key`, `get-email`, `age-equal` and `age-range`, and redb 0.58 to 0.88 times in all of them but `get-email`, where the two are level; on an Apple M1 Max, DaruDB was ahead of LMDB in all of them but `age-range`. A profile of `get-key` repeated 30 times on an EPYC 7763 (`probe-profile.yml`), before the changes below:

| Function | Own time | What it is |
| --- | --- | --- |
| `NodeRef::rank` and its closure | 24% | The search inside a node, comparing keys as bytes after their four-byte heads; the closure is reading the key in the page, where the cache misses are |
| `schema::typed::read_record` | 18% | Decoding the record into the struct, field id by field id; most of it is the first touch of the record |
| `Loader::load` | 18% | The page cache: the hash table probe, the `Arc` clone, the mutex |
| `malloc`, `free`, `from_utf8` | 6% | The struct's strings, which the other stores' structs allocate too, and the encoded key |

LMDB spends 52% in its node search and reads mapped memory with no cache. redb 4.3 does not map its file: it reads through a striped `RwLock` cache of `Arc<[u8]>` pages, as DaruDB does, compares its `u64` keys as integers, and reads the record written by hand, with fixed places.

Done (2026-10-11):

- **A read transaction keeps the roots it looks keys up in** (`KeptRoot` in `txn/read.rs`), and finds the tree there rather than under the lock every read transaction of the commit shares: on an EPYC 7763, `get-key` 5 to 9% and `get-email` 6 to 9% faster; on the M1 Max 3.5% and 7%.
- **A key is encoded on the stack for a lookup** when it is an int or a string of up to 61 bytes with no zero in it (`key::with_encoded`): 2 to 4% on Linux, 5 to 7% on the M1 Max, whose allocator zeroes what it frees.

Tried and not kept:

- `parking_lot::Mutex` for the page cache: within ±1% on Apple silicon, within ±1.4% on every row on an EPYC 9V45, and on an EPYC 7763 −5% on `get-key` and +5% on `city-scan`, with the ranges overlapping. No lock type removes the atomic operations of a lookup.
- A per-thread memo of 64 branch nodes in front of the page cache (`memo.patch`, in the branch's history): `get-key` −5.4%, but one-object deferred commits +14%, because a writer cannot take a branch the memo holds and copies it, and `age-equal` +3.8%, since storing into the memo on a miss costs atomics too. The kept roots avoid that by ending with the transaction.
- Decoding in layout order: a fast path in `FieldReader::next` for a record whose next field is the layout's next, small enough to be inlined into a derived type's `read`, alone and with the scalar readers inlined as well. On the M1 Max, `get-key`, `get-email` and `age-equal` were 3 to 5% slower with either, and the other rows level. With one key read over and over, so that every byte stays in the caches, decoding is about 80 of a lookup's 230 ns there, and `FieldReader::next` 30 of them; but the benchmark reads each record cold, and the first touch of it, not the matching, is what `read_record`'s time is. In that loop a third of the time is the allocator of macOS, for the struct's three strings, whose `free` reads the clock on every call.

Directions not tried yet:

- **The levels below the root.** Random keys share the branch below the root once in about twenty lookups, so keeping the path, as `btree::read::Seeker` does for a query, saves little beyond the root for them.
- **Integer primary keys.** An int key of the records tree is three or four bytes, which the head after the node's prefix covers whole, so a search reads a key only to confirm the head it found; a node whose keys all end inside their heads could skip that read. The record's cell is beside the key, so the cache miss would move rather than go.
- **Readers without reference counts.** Readers that borrow nodes for the life of the transaction, reclaimed through epochs or hazard pointers, so that a lookup takes no lock and touches no count. It needs `unsafe` beyond `sys/`, or a crate whose API does, and it touches every read path.

### 2. Updates in Rust

LMDB took 0.77 times DaruDB's time in `update` on an EPYC 7763, while DaruDB was 12% ahead on an EPYC 9V45, both with the engine as it is now. A profile on the M1 Max (2026-10-11, the A/B harness's `putrep`, typed puts that replace an object with a new age) puts 84% in the put's store: the inserts into the trees, the record's and the age index's new entry, take about half, of which the leaves' search is 17% and their insert 15%, mostly moving memory: the heads and slots after an entry, and compacting a leaf whose free room is in cells removals left. Removing the old entry of the age index takes 14%. A leaf compacts about once in six of these inserts, but compacting is cheap: moving runs of adjacent cells together rather than one cell at a time changed puts, updates and deletes by less than 1%, so putting an entry into the cell a removal left, to save compacting, would gain as little. What is left is the shifting of the heads and slots after each entry, which a leaf of a few hundred entries makes on every insert in its middle.

### 3. Small commits write scattered pages

A one-object deferred commit makes about ten write calls for eleven pages, against LMDB's seven, and a sync commit writes them scattered, where SQLite appends to its log. The commit writes its pages where the free pages are; two prototypes that grew the file to place them in runs made it a third larger and slowed reads and writes by 7 to 27% once it outgrew the page cache (`CLAUDE.md`, "A small commit writes its pages where the free pages are").

Measured on 2026-10-11:

- **Where a deferred commit's time goes on Linux** (`probe-profile.yml` on an EPYC 9V74, by system call): of 34 µs, the writes take 8.4 µs, against LMDB's 6.4 µs of its 12 µs, and the writer lock, the lock tests and the header read 2.8 µs. The rest is the processor's work in user space: encoding and sealing the pages, with the page hash about 5%, changing three trees, and settling the allocator trees. Writing the pages in runs could save the 2 µs the writes cost more than LMDB's, not the gap.
- **io_uring is slower, not faster** (`probe-uring.yml`): ten scattered 4 KiB writes into the page cache took 12 µs as ten `pwrite` calls and 27 to 46 µs submitted together, on EPYC 7763 and 9V74 runners, since io_uring hands buffered writes to its worker threads. Not worth a dependency, raw system calls or a fallback.

- **A sync commit's barrier costs more after scattered writes** (`probe-syncscatter.yml`): twelve 4 KiB pages written over a 64 MiB file and then synced with `fdatasync` took 570 to 600 µs on EPYC 7763 runners, whose disk is `sda`, and 178 to 205 µs written as one run; on EPYC 9V45 runners, whose disk is `nvme`, 264 to 300 µs against 183 to 234 µs. Appending them to a file that grows took longer still, since the file's size changes. So on the first kind of runner, a sync commit's scattered pages, not its writes, are what puts SQLite's log ahead in `insert-sync`.

- **Starting the writeback early does not help** (`probe-syncscatter.yml`, `sync_file_range` with `SYNC_FILE_RANGE_WRITE`): starting each scattered page's writeback as soon as it was written, or the whole file's in one call before the barrier, left the sync as long as before on EPYC 7763 runners (573 against 590 µs for twelve pages) and made it longer on others. What costs is the number of separate writes the disk makes, not when they start.

Directions not tried yet:

- **A sync commit's pages in runs.** A small sync commit whose pages lay in one or two runs would make its barrier a third as long on `sda` runners. The pages are allocated as the transaction changes nodes, before it knows whether it commits with a sync, so either every small commit would place its pages in runs, with the file growth the prototypes above ran into, or a sync commit would move its pages at commit time, which changes how the allocator trees settle.
- **A bounded area for small commits.** Keep a few runs of free pages near the end of the file for small commits, grown by a fixed amount, so that their pages land in runs without the file growing as the prototypes did. Worth at most the 2 µs above.

### 4. The other system calls of a commit

Besides the writes, a commit takes and releases the writer lock (one `F_SETLK` each), tests the turn lock (`F_GETLK`) and tests the snapshot bytes (`F_GETLK`). The test of the file's length is gone (`98be56b`).

- **The turn lock test costs about 1%.** Removing it outright (`noturn.patch`, in the branch's history) made a deferred commit on an EPYC 7763 1.4% faster, with overlapping ranges. Testing it once a millisecond would keep the turn's bound and save less. A single call that takes the writer lock and fails while the turn is claimed would need the writer byte moved, since the bytes beside it are the open lock and the recovery lock, and moving it breaks running beside older versions. Left as it is.
- **The snapshot test stays.** A write transaction reclaims the young part of the group the commit before it retired (`Locks::young_reclaimable` in `lock/mod.rs`), which is new at every commit, and only the operating system can say whether a snapshot of another process lies between the durable commit and that group: one `F_GETLK` per commit in the window. A transaction with no group to reclaim makes none already.

### 5. Queries without an index

LMDB scans several times faster in Rust, redb about twice as fast in Rust on an EPYC 7763 but level on a 9V45, Hive up to two and a half times in Dart, and SQLite 6 to 15% faster in Node.js, Dart and Python, before the changes below. A DaruDB record tags each field with an id and a type, so a scan steps over the fields before the one it tests; the stores ahead read fixed places, or, as SQLite, add up the sizes the record's header lists. Changing the record is a new format version, with a migration (`CLAUDE.md`, "The file format is released").

Done (2026-10-11):

- **A number is compared with a number of its type directly** in a filter and a sort, without a `Value` made for it: `top-score` 7 to 11% faster on Linux runners.
- **Field marks per leaf, kept beside the page cache** (`Marks` in `btree/load.rs`): the first scan of a leaf marks where the plan's first field lies in each record, and the next one reads it there. On Linux runners, `city-scan` 16 to 21% and `top-score` 19 to 23% faster on top of the comparison, on the M1 Max a quarter. Through Node.js and Python on the M1 Max, 22 to 29%.
- **Short strings compared by words** in an equality, rather than through a call to compare memory: through the Python package, whose build had made the comparison a call, `city-scan` a tenth faster on the M1 Max; level in the Rust harness on Linux. Forcing the filter's functions inline as well made the Rust harness's `top-score` 3 to 4% slower on an EPYC 7763, through what moved where, and was left out.
- **An equality made in the direct test** (`direct` in `query/run.rs`), rather than through a call to `passes` for every record: `city-scan` 6% faster on an EPYC 9V45 and on the M1 Max, the other rows within the 6% that two builds of one engine differ by.
- **Marks made in the walk's own loop and kept together when it ends**, with no lookup on the first walk of a tree by a field: the first run of a scan took 14% (`city-scan`) and 8% (`top-score`) longer with marks than without on an EPYC 7763 when a pass over each leaf of their own made them and each leaf's were kept under the lock in turn, and takes 2 and 4% longer now. The rows `city-scan-first` and `top-score-first` show it.

Tried and not kept:

- **Marks in `LoadedNode`.** As fast for scans, but the 16 bytes took the node past one cache line, and `age-equal` was 3% slower on an EPYC 7763 in two runs, as it was with the node grown and the marks unused.
- **Learning marks through the record view as the filter reads each field**, handing a cell down through the query's layers: the first run was slower still, since the walk's closures grew for every record, and the second slower too. Making them in the range walk's loop, from the value it already holds, is what was kept.
- **Marks made from a tree's second walk on**: the first run as fast as without marks, but the rows of ten runs 5.6% slower on an EPYC 7763, since the second run pays for them.
- **Hashing the marks' keys with SipHash** took a tenth of a first scan; they are hashed as the page cache's keys are.
- **Batching the field search a leaf at a time** (a micro-benchmark): no faster than record by record on the M1 Max, whose processor already overlaps the records' searches.
- **`meets` inlined into the walk's closure**, on top of the equality: on the M1 Max, `city-scan` a point faster than with the equality alone, and `top-score` 2% slower.

Directions not tried yet:

- **The walk's layers.** A scan hands each record through the range walk, the plan's walk, the gathering of results, the filter's tree and the record view, which took 30% of a profile of `city-scan` on an EPYC 7763 before the marks, more than finding the field took (22%). A filter that is one test of one field now skips the filter's tree, which took 7 to 10% off `city-scan`; making `Reader::any_in` generic over its test instead made `city-scan` 12% slower on the M1 Max, through what the compiler inlined. What is left is the range walk, the gathering of results and the record view: in a profile of `city-scan` with marks on the M1 Max, `meets`, which reads the field at its mark and tests it, took 46%, the leaf's walk (`visit_leaf`) 17%, `found`'s closure 13% and the call to `passes` 8%, which the equality now skips.
- **The first run.** A scan that runs once still pays 2 to 4% for the marks: a vector for each leaf, and the batch kept at the end.

### 6. A lookup through a unique index in Node.js

SQLite took 0.81 to 0.89 times DaruDB's time in `get-email` through the Node.js package. A profile on the M1 Max with the addon's symbols (2026-10-11) puts 81% of a lookup in the native call: the two tree lookups, the email index and the records, take 60%, and finding each tree by name under the lock every read transaction shares 5%. Decoding the record into an object takes the 19% left, in JavaScript. The kept roots and the key on the stack took 4% off through the package on the M1 Max; on Linux they took 8% off the Rust harness's `get-email`.

### 6b. Reads and commits through the Python package

On EPYC 7763 runners a one-object deferred commit took 55.6 µs through Python and 37.4 µs through Rust (run 38121829420). A profile on a runner (`probe-python.yml`) put a fifth of it in the interpreter, about 11 µs, running the write block, and most of the rest in the engine and the kernel as in Rust.

Done (2026-10-11):

- **Reads through a shape** (`darudb::__binding`, `Layout::read_record` in `packages/python/src/values.rs`): the binding copies the records out of the engine and reads each straight into Python values, rather than into an engine `Object` it then built the instance from, which was more than half of reading an object through an index. On the M1 Max, `age-equal` 21% and `age-range` 28% faster through the package, `get-key` 15% and `get-email` 13%.
- **The write block** no longer imports the asynchronous module or takes its lock while no asynchronous write runs: a one-object commit 4% faster on the M1 Max.
- **Writes as records** (`CollectionWriter::write_shaped`, `Layout::held` in `packages/python/src/values.rs`): an object of a class whose fields all hold scalars has its attributes read into scalars with the GIL, and the engine encodes the record through the class's shape without it, as a derived type's `write` does, rather than an engine `Object` made by name. On the M1 Max, `insert-bulk` 13 to 15% faster through the package; `insert-deferred` within the 6% that two runs of one build differ by, since the interpreter and the commit are most of it. A class with a list, an embedded object or a link still goes through an `Object`, since the engine reads such a record into one to check it.

### 7. The benchmark itself

- **The runner decides more than the code.** Which store leads the commit rows, and even the scans in Rust, changes with the kind of runner. Options: run all four languages in one job, so that the tables at least share a machine; or run each language on several runners and show the kind with each table, as `probe-machines.yml` does.
- **Rows that spread.** A Dart sync commit spread 599% between passes in one run, and in run 38128026467 DaruDB's update spread from 4.6 to 8.7 µs an object in Rust. The workflow and `bench/run.mjs` take nine passes rather than five since then, whose median stays put through four slow passes rather than two. A runner that is slow through a whole run moves every store's row alike, as Node.js's update and delete were in that run, slower for every store than in the run before on a slower processor, and more passes do not steady that; only another run does.
- **Realm in Dart.** Realm is measured through Node.js only. Its Dart package downloads its library with `dart run realm install` and is built for Flutter; adding it would need that step in the workflow.
- **`update` in Rust and Dart** (measured 2026-10-11 on the M1 Max). In Dart, `update` of the age took 0.97 times the time of writing the object back with `put`, so the Dart pass sets the field with `update` now, as the Node.js and Python passes do. In Rust it took 1.15 times as long, since `TypedWriter::update` takes its changes as a dynamic `Object`, made and matched by the field's name, where `put` writes the struct's record as it is, and the Rust pass still writes the struct back. A typed update, whose changes name the field through the type, would close that.
- **Scans run ten times in a pass.** `city-scan` and `top-score` scan the collection ten times, and every run after the first reads the field from the marks the first left. The rows `city-scan-first` and `top-score-first` (`FIRST_RUN` in each harness) time the first run on its own, which shows what a query that runs once on a new handle takes.
