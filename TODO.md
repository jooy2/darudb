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

Phase 3 is done: the open, writer and snapshot locks of [design/locking.md](design/locking.md), several processes on one file, and network file systems refused. `src/processes.rs` fuzzes it: four worker processes at a time, each writing and reading through several handles and threads, with random ones killed. It passes 300 kills on a plain file and on an encrypted one. Beginning and ending a read transaction went from 38 nanoseconds to about 2 microseconds, which two reads of the header and a lock call cost.

## Known gaps

Confirmed by reading the code against the specification; no test reproduces them yet.

- **The unsynced window is counted per process.** The page and time limits bound each process's own deferred commits. When a process with deferred commits dies, nothing ends its window until another process syncs or makes a sync commit, and the windows of several processes add up.
- **No no-replace rename.** On a file system without links, `storage/create.rs` goes straight to creating the file in place under the recovery and open locks, where `design/commits-and-recovery.md` would first try `renameat2`, `renamex_np` or `MoveFileExW`.
- **Windows identifies a file by its canonical path**, not by its volume serial number and file index as `design/locking.md` asks. Two hard links to one database are two instances there, and their locks keep them apart as two processes' locks would.

## Open questions

- **Final minimum OS and runtime versions.** If Windows 7 and 8 are needed, the Tier 3 Rust targets have to be built by us.
- **JavaScript runtimes beyond Node.js**: Electron (whose main and renderer are themselves several processes), React Native, Bun, Deno.
- **Minimum Dart and Flutter versions.** Build hooks need Dart 3.10 or Flutter 3.38 at least.
- **Encryption API in the bindings**: how Node.js and Dart take a key or a password, and whether they offer the operating system's keystore. The Rust API exists.
- **Choosing the page cipher.** A new encrypted file gets the page cipher that suits the processor creating it, and the caller cannot pick one. Whether to offer that, for a file that moves between very different machines, is open.
- **Query API form**: string queries, a builder, or both.
- **Schema migrations**: how they are declared and when they run.
- **File format versioning**: the forward and backward compatibility policy, and whether an older file is upgraded on open or by an explicit call. The design settles only the pre-release rule: no migrations until the first release.
- **Busy timeout**: how long opening and writing wait for another process by default before failing with `BUSY`.
- **Cost of beginning a read.** About 2 microseconds against 38 nanoseconds before phase 3. Whether to cut it, for example by skipping the second read of the header and the lock call when another read transaction in the process already holds the snapshot, is open.
- **The barrier after another process's commit.** A writer that does not know which selector a power cut would bring back issues a barrier before its record, so processes that take turns committing pay one barrier each time the writer changes. The turn lock waits 50 milliseconds for that reason: at 5, two processes committing in tight loops made a twentieth of their commits. Whether the writer can tell that selector from the records instead, and skip the barrier, is open, and would let the turn come sooner.
- **Unsynced window limits**: how many pages and how much time deferred commits may accumulate before the engine issues a barrier of its own.
- **iOS App Group containers**: iOS terminates a suspended app that holds a file lock in one, and the open lock is held while a database is open. Whether to offer a mode for such apps that does without it.
- **Performance goal**: the benchmark workloads, and the durability settings to compare at.
- **Default page size.** 4096 bytes today, which is a placeholder rather than a measured choice.
- **Minimum supported Rust version.** `rust-version` is 1.85 today, the first release with the 2024 edition. Whether to hold it there is open.
