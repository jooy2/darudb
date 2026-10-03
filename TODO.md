# What is not done yet

The roadmap, the questions still open, and the work that outlived a session. A note from a finished session is otherwise lost, so what is left is written down here rather than remembered.

Three rules keep it accurate:

- **Nothing goes here that a check already enforces.** A test that fails when something is missing is a better record than a line here.
- **Confirmed and unconfirmed are marked apart.** A line that says where the code is has been read. A line marked "reported" has not been reproduced yet.
- **A decision is not work.** When an open question below is decided, it moves into [CLAUDE.md](CLAUDE.md), into the section it belongs to, and leaves this file.

## Roadmap

Tentative, like the architecture it builds. Each phase ends on a criterion a test can show, not on a date.

| Phase                   | Work                                                                                   | Done when                                                                                      |
| ----------------------- | -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| 0. Design spec          | Documents for the file format, the commit and recovery protocol, and the lock protocol | Written before the code they describe. This spec decides how stable the file is.               |
| 1. Storage kernel       | Page file, copy-on-write B+tree, three commit slots with checks; one process only      | No data lost across thousands of repeated `kill -9` and simulated power cuts                   |
| 2. Encryption           | Page AEAD, DEK and KEK, KDF                                                            | The same suite passes with encryption on and off, and the slowdown is measured                 |
| 3. Several processes    | The file range lock protocol, cache invalidation                                       | Fuzzing that mixes concurrent reads and writes from several processes with forced kills passes |
| 4. Objects and queries  | Schema, indexes, query IR, migrations                                                  | Benchmarks against established embedded databases at the same durability settings              |
| 5. Bindings and release | Dart build hooks, napi-rs, per-platform prebuilt binaries                              | A CI matrix that includes the oldest supported operating systems                               |
| 6. Tools                | Integrity check, salvage, backup, compaction                                           | Data recovered from deliberately damaged files                                                 |

Phase 0 is done: [design/](design/README.md) holds the accepted file format, commit and recovery protocol, and locking protocol.

Phase 1 is done: the file format for plain files, the copy-on-write B+tree, sync and deferred commits, recovery, and the crash suites, which pass at thousands of simulated cuts and hundreds of real process kills. `examples/kernel_bench.rs` measures the kernel.

Phase 2 is done: page encryption, the key block, passwords, key changes, and the crash suites running with encryption on and off. `examples/kernel_bench.rs` measures both kinds of file. On one Apple silicon machine, with XAES-256-GCM, a read that misses the page cache went from 4 to 6 microseconds and reading a 256 KiB value from 41 to 141.

Phase 3 is done: the open, writer and snapshot locks of [design/locking.md](design/locking.md), several processes on one file, and network file systems refused. `src/processes.rs` fuzzes it: four worker processes at a time, each writing and reading through several handles and threads, with random ones killed. It passes 300 kills on a plain file and on an encrypted one. Beginning and ending a read transaction went from 38 nanoseconds to about 2 microseconds, which two reads of the header and a lock call cost, and back to about 1 when the process keeps the snapshot's lock for the next read transaction to join.

Phase 4 is under way. The object layer of [design/objects.md](design/objects.md) stores a schema, writes and reads objects with their indexes in step, migrates a file from one schema version to the next, and runs queries built in Rust through the primary key or an index when one fits. The query language parses into the same queries. The crash suite and the multi-process suite write objects too. `examples/object_bench.rs` measures the object layer; on one Apple silicon machine, a deferred commit of one object with two indexes takes about 95 microseconds against 56 for one value in the kernel, reading by key 1.4, and a walk of every object about 0.6 per object. The Node.js package has the object API, synchronous and asynchronous, and `npm run bench` measures it with the same workloads. Still to come: the comparison with other databases that ends the phase.

Phase 5 is under way. The Node.js package is written, and `.github/workflows/release.yml` builds its prebuilt addon for every target in its `napi.targets`, tests it on Linux, macOS and Windows, and makes the per-platform packages with their licence notices; it has not run on GitHub yet, and nothing is published. The first release waits for the maintainer, and its dry run on GitHub is made just before it. The crate packages and builds with `cargo publish --dry-run`. Still to come: the Dart package, and the CI matrix of the oldest supported operating systems that ends the phase.

Phase 6 is done, and the maintainer accepted [design/tools.md](design/tools.md) on 2026-09-29: the integrity check, backup, compaction and salvage, in Rust and in the Node.js package. Salvage's tests rescue files damaged on purpose, in leaves, overflow values, commit records and the static header, plain and encrypted, and one damages random pages of random files and checks that nothing in the result is a value no commit wrote. The crash suites run the integrity check after every cut.

## Known gaps

Confirmed by reading the code against the specification; no test reproduces them yet.

- **The unsynced window is counted per process.** The page and time limits bound each process's own deferred commits, so the windows of several processes add up. A window whose process died is ended by a process that reads the file or opens it; one that only holds the file open does not notice.

## Open questions

- **Encryption API in Dart, and keystores.** Node.js takes a key or a password as Rust does. How Dart takes them, and whether the bindings offer the operating system's keystore, is open.
- **Scattered writes of small commits.** A deferred commit of one object with two indexes writes about eleven pages in as many calls, since the pages it takes are the scattered ones commits before it gave up; on one Mac, eleven scattered pages took 21.6 microseconds to write and eleven consecutive ones 4.6, about half of the commit. Taking a run of 16 free pages, or growing the file while less than a quarter of it was free, made those commits slower and the file a third larger: fresh pages end the window's reuse of the pages it wrote, and runs do not form, because each commit leaves the leaves it wrote, which live long, among the path pages it gives up. Keeping pages that are rewritten soon, those whose last version the window wrote, in runs apart from the rest cut a one-insert commit's calls from eight to two and its time by a tenth, with the file 7% larger; but among other workloads the file grew by a third, past the page cache, and reads and writes both slowed by 7 to 27%. Whatever lets small commits write in runs has to keep the file from growing, which the maintainer made the condition on 2026-09-29.
- **Rotating the data key.** Changing the key or the password rewraps the data key and re-encrypts nothing, so the data key is the same for the life of a file: compaction works in place, and backup and salvage copy the key block as it is. Writing a file under a new data key, as a backup or salvage option or as a tool of its own, is not decided.
- **iOS App Group containers**: iOS terminates a suspended app that holds a file lock in one, and the open lock is held while a database is open. Whether to offer a mode for such apps that does without it is decided with the Dart package, on a real device.
