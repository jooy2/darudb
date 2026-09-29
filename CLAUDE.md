# Working in this repository

What a reader has to know before changing anything here, and where to find the rest. [CONTRIBUTING.md](CONTRIBUTING.md) has the full procedure and every command; this file is the map, the requirements and the design decisions. [TODO.md](TODO.md) is the roadmap, the open questions and the work that outlived a session. [design/](design/README.md) holds the specifications of the file format, the commit and recovery protocol, the locking protocol, and the object layer.

Decisions below carry one of two marks:

- **[Decided]**: the maintainer has made this call. Do not reopen it without being asked.
- **[Tentative]**: the direction agreed in discussion, whose details may still change. **Check with the user before changing one or pinning down its details.** When an open question in `TODO.md` is decided, move it into the section here that it belongs to.

## What this is

DaruDB is an embedded database that keeps an application's data in one local file. The engine is written in Rust and shipped to Rust, Node.js and Dart through thin bindings. The work goes to the Rust engine and the Node.js binding first; Dart follows.

The five requirements, in the order the maintainer ranked them. **[Decided]**

1. **Performance.** Reads and writes faster than the embedded SQL engines applications usually reach for, measured at the same durability settings. Whether that is reached is for benchmarks to say, and until they exist, nothing here claims it.
1. **Encryption.** Encryption of the whole database file, with authentication, and keys handled properly.
1. **Stability.** Rarely crashes, the file does not break easily, a broken file can be recovered, and several processes can use one file at the same time.
1. **Compatibility.** Runs on many operating systems, from old releases to the latest. The file format is versioned, and every format version and every schema version comes with a migration from the one before.
1. **Developer convenience.** Queries are easy to write, and so is a schema.

**How performance is judged** (decided 2026-09-29): with the workloads of `examples/object_bench.rs`, at both durability levels, sync commits against settings that synchronize every commit and deferred commits against relaxed settings, in Rust and through the Node.js package. The goal is reached when most workloads are ahead, and every one that is not has its cause written down: what makes it slower, and whether it can be fixed or is the cost of a design decision. The comparison harness and its results stay outside the repository, since the repository names no other product.

The project is written and maintained with coding agents, now and later. Keep the structure easy to pick up cold: one responsibility per module, a doc comment at the top of each module saying what it owns and which invariants it keeps, tests next to the behaviour they check, and no coupling that a reader of one file cannot see.

## Rules that hold everywhere

- **Never name another database product in this repository.** Not in code, comments, documentation, commit messages, tests or benchmark names. DaruDB goes its own way, and its designs are described on their own terms. **[Decided]**
- **Our own code from the start.** No existing database engine is embedded with a plan to swap it out later, and no existing database is forked. Studying other designs is fine; copying them is not. **[Decided]**
- **Everything in the repository is English**: code, identifiers, comments, commit messages, error messages. The Korean pages under `docs/ko` are the exception, and the changelogs stay English even on the Korean site.
- **Prose is plain and explains why.** Comments and documentation say what a decision costs and what the alternative was, in complete sentences, with no emoji and no decoration.

## Layout

| Path            | What it is                                              | Entered with                                                    |
| --------------- | ------------------------------------------------------- | --------------------------------------------------------------- |
| `crates/darudb` | The engine and the Rust API, the crate `darudb`         | `cargo test -p darudb` from the root                            |
| `packages/node` | The Node.js binding, the npm package `darudb` (napi-rs) | `npm install`, then `npm run build`, `npm test`, `npm run lint` |
| `docs`          | The VitePress site, shared by every package             | `npm install`, then `npm run dev`                               |
| `design`        | The engine's specifications, in English only            | Read before changing `format`, `storage`, `txn` or `lock`       |

**The root is a Cargo workspace and nothing else.** `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml` are there; there is no root `package.json` and no npm workspace. Each JavaScript folder installs and runs on its own, so check which folder a command belongs to before running it. `packages/node` is both an npm package and a member of the Cargo workspace.

`packages/dart` is planned and will hold the Dart package and its Rust glue crate, the same way `packages/node` does.

### The engine, module by module

`crates/darudb/src` is layered, and a module only uses the modules below it. Keeping that one-way is what lets a layer be read, tested and replaced on its own.

| Module        | Owns                                                                                     | State            |
| ------------- | ---------------------------------------------------------------------------------------- | ---------------- |
| `database.rs` | `Database`, the public handle: open, create, begin a transaction, close                  | Phase 1          |
| `options.rs`  | `OpenOptions`, and validating what the caller asked for                                  | Phase 1          |
| `error.rs`    | `Error`, `Result`, and each failure's stable code                                        | Phase 1          |
| `txn/`        | Read and write transactions, the commit, recovery                                        | Phase 1          |
| `instance.rs` | The one shared instance of each open file in the process: header, writer gate, snapshots | Phase 1          |
| `space.rs`    | Free space during a write transaction: allocation, release, reclaiming                   | Phase 1          |
| `btree/`      | The copy-on-write B+tree: reads, changes, commit-time encoding, verified loading         | Phase 1          |
| `storage/`    | How bytes reach the disk: positional I/O, the pager, the page cache, file creation       | Phase 1          |
| `format/`     | What bytes on disk mean: the layouts of `design/`, objects in `format/object`. No I/O    | Phase 4          |
| `lock/`       | Cross-process coordination through file range locks                                      | Phase 3          |
| `crypto/`     | Page encryption, key wrapping, key derivation. No I/O, like `format`                     | Phase 2          |
| `sys/`        | The operating system's calls the standard library lacks; the one module with `unsafe`    | Phase 3          |
| `schema/`     | Declared schemas, migrations, and objects written with their indexes in step             | Phase 4          |
| `query/`      | The query IR, the builder and the query language, choosing an index, running a query     | Phase 4          |
| `tools/`      | Integrity check, salvage, backup, compact                                                | Phase 6          |

From the bottom up: `format`, `crypto` and `sys`, then `storage`, `btree`, `space`, `lock`, `instance`, `txn`, `schema` and `query`, `tools`, and `database` on top. `lib.rs` re-exports the public surface and nothing below `database`'s level leaks into it.

Tests sit beside what they test, plus these places that test the whole engine:

- `src/crash.rs`: the crash suite. Random transactions on the simulated disk of `storage/sim.rs`, cut by power failures and process deaths, then reopened and compared with the history of commits, with the integrity check of `tools/check.rs` and an accounting of every page written apart from it, which fails the run if it finds what the check missed. Half the runs use an encrypted file. Object runs write collections instead and check the indexes against the objects after every cut, and migrations are cut at every step. `DARUDB_CRASH_SEEDS` makes it longer.
- `tests/process_kill.rs`: real child processes killed while they commit. `DARUDB_KILL_ROUNDS` makes it longer.
- `src/processes.rs`: the multi-process suite, the phase 3 exit criterion. Worker processes of the test binary read and write one file through several handles and threads while random ones are killed and new ones start; then the integrity check, and every commit a worker reported. Each commit also writes an object under a unique index the workers contend for, and readers check the indexes against the objects. One run uses an encrypted file. `DARUDB_PROCESS_KILLS` makes it longer.
- The lock tests in `lock/tests.rs` run a second process through `testing::Helper`, since a process never conflicts with its own locks on Unix-like systems.
- `tests/transactions.rs`, `tests/open.rs` and `tests/objects.rs`: the public API on real files.

`examples/kernel_bench.rs` measures the storage kernel: commits, bulk writes, reads and large values, on a plain file and on an encrypted one, and opening a file with a password. `examples/object_bench.rs` measures the object layer: inserts, reads by key and by index, queries with and without an index, updates and deletes; its doc comment spells out the workloads, so that they can be run against other databases outside this repository. Both are for comparing two builds on one machine, and a performance change quotes their numbers from before and after.

## Scope **[Decided]**

- **Targets**
  - Rust: the crate is a public API, not only the engine behind the bindings.
  - Node.js: servers and desktop applications, in Electron's main process too (decided 2026-09-29). Other JavaScript runtimes are not targets.
  - Dart: Flutter apps, and Dart servers and command-line tools.
- **Out of scope**
  - Browsers and websites (WASM, IndexedDB, OPFS and the like).
  - Remote sync. This is a local file database.
  - Reactive or live-object notifications, for now. They were inconvenient in practice and a frequent source of crashes in the designs that had them.
- **Queries**: filtering, sorting and links between objects. Joins at the level of a SQL engine are not a v1 goal, though more is welcome later.
- **Several processes on one file must be stable.** Designs that coordinate processes through shared memory have been effectively limited to one process at a time, and fixing that is one of DaruDB's central tasks.

## Architecture

The storage engine, crash safety, several processes and encryption are **[Decided]**: the maintainer accepted the specifications in [design/](design/README.md) on 2026-09-23, and those documents are the authority on every detail below. The rest of this section is **[Tentative]**.

```text
Rust API         Node.js binding (napi-rs)         Dart binding (dart:ffi + build hooks)
    └─────────────────────── Rust engine ───────────────────────┘
  Objects, schema, queries   (query IR, indexes, migrations)
  Transactions, snapshots, cross-process locks   (OS file range locks)
  Copy-on-write B+tree
  Page I/O, page cache, checks, AEAD encryption
```

- **One engine, thin bindings.** Query semantics live only in the engine. A binding builds a query IR and passes it in, so every language behaves the same way.
- **Why Rust**: memory safety (use-after-free crashes are the class of bug this project most wants to leave behind), good distribution through Dart build hooks and napi-rs, and C-level performance.
- **What Rust does not prevent**: durability bugs such as a wrong fsync order or a mistake in the commit protocol. Only tests catch those, which is why the storage and multi-process suites are the heaviest in the repository.

### Storage engine **[Decided]**

Specified in [design/file-format.md](design/file-format.md).

- **Copy-on-write B+tree.** A committed page is never modified in place.
- **A header with a selector byte and three commit slots.** One slot holds the published commit, one the durable commit, and the writer fills the third, so no record anyone may need is ever overwritten.
- **Every pointer carries the page number, the transaction id that wrote the page, and the page's check** (32 bytes). Each page is verified against its pointer before use. The check is XXH3-128 in a plain file and the AEAD tag in an encrypted one.
- **No mmap; positional reads and writes (`pread` / `pwrite`, and `pwritev` for a commit's runs of pages) through our own page cache.** Memory-mapped files are hard to prove sound in Rust, because another process can change the mapped bytes under a live reference, and they conflict with both multi-process access and encryption. The cost is the zero-copy read path mmap would give, so the performance goal has to be proven by benchmarks against this design.
- **Page size**: a power of two from 4096 to 65536, recorded in the header; 4096 by default, which the benchmarks chose (decided 2026-09-27). Larger pages made scans, counts and deletes faster, but a lookup in a file larger than the page cache slower, since a miss reads and checks a whole page, and a small commit slower, since it writes whole pages; an application that mostly scans can choose more with `OpenOptions::page_size`. It is independent of the operating system's page size, which is never assumed (Android now uses 16 KB pages).
- **Every page reserves a 24-byte nonce field, encrypted or not**, so a tree has the same shape either way and one test suite covers both.
- **Objects on the kernel** (**[Decided]**, specified in [design/objects.md](design/objects.md)): a tree per collection from the encoded primary key to the object's record, and a tree per index, all under names beginning with `0x00`. Records hold field ids rather than names, in a format the bindings exchange with the engine.

### Crash safety and recovery **[Decided]**

Specified in [design/commits-and-recovery.md](design/commits-and-recovery.md).

- **Five platform assumptions and no others**: a one-byte write is atomic, a write changes only the bytes it names, a successful sync makes earlier writes durable, byte-range locks work and die with their owner, and the file system is local. [design/README.md](design/README.md#what-the-engine-assumes-of-the-platform) states them exactly.
- **A sync commit costs one barrier.** The barrier is the commit point; flipping the selector afterwards publishes a commit that is already durable, so readers never see one that a power cut could undo.
- **Deferred commits** are published without a barrier and become durable at the next one: the next sync commit, `Database::sync`, closing the database, or the window's limits on pages and time. The page limit counts each page the window wrote once, however often it was written (decided 2026-09-26), since deferred commits reuse the window's pages. The limits are 16,384 pages and one second by default (decided 2026-09-29), and `OpenOptions` changes them. A process that finds another process's commit still unsynced a time limit after it first saw it ends that window, since its process may have died (decided 2026-09-29). A power cut undoes them only from the newest backwards; a process crash loses none.
- **Three records protected at every commit**: the published one, the durable one, and the one a power cut would make recovery trust without checking. When no slot is left, the commit issues a barrier first.
- **Recovery** runs in the first process to open the file. It adopts the newest commit that is either published with the unsynced bit clear or passes checking, where checking reads only the pages written since that commit's durable transaction id.
- **A failed barrier is not retried.** The commit fails with `SYNC_FAILED`, and the handle is unusable until the file is reopened.
- **A new database is written to a temporary file and moved into place without replacing anything**, so the path holds either nothing or a complete database.
- **Tools to ship with the library**: an integrity check, salvage (build a new file from the pages whose checks are valid), online backup, and compaction. [design/tools.md](design/tools.md) specifies them, and the maintainer accepted it on 2026-09-29, with salvage starting from the newest commit record and filling only what it cannot read from older pages.
- **Not supported**: network file systems (NFS, SMB). They are detected and refused with `UNSUPPORTED_FILE_SYSTEM`.

### Several processes **[Decided]**

Specified in [design/locking.md](design/locking.md).

- **No mutexes in shared memory.** A process that dies holding a shared-memory mutex leaves it held, recovering from that needs robust mutexes that not every platform has, and a lock file with a memory layout in it breaks between processes of different architectures.
- **Only operating-system byte-range locks** (`fcntl` on Unix, `LockFileEx` on Windows), on bytes from 2^62 up, where no data ever is: an open lock, a writer lock, a recovery lock that a process holds while it opens the file, a turn lock, and one byte per snapshot. When a process dies, the operating system releases its locks, and nothing else needs cleaning up.
- **One operating-system handle per file per process**, shared by every `Database` object for that file, because closing any descriptor drops all of a process's POSIX locks.
- **Readers take no header lock**: they read the header, register their snapshot, and read it again.
- **The writer reclaims pages** only from groups that no registered snapshot and no possible recovery can still reach.
- **The page cache is keyed by page number and check**, so a stale entry never matches and nothing has to be invalidated when another process commits.
- **Concurrency model**: one writing process at a time and any number of readers. Waiting for the writer lock past the busy timeout, 5 seconds by default (decided 2026-09-29), fails with `BUSY`.
- **Writers take turns.** A writer that has waited 50 milliseconds claims the turn lock, and every other writer lets it go first, so a process that commits in a tight loop cannot keep the others out. The wait is long enough that the lock seldom passes back and forth, since each pass used to cost the next commit a barrier; now it costs one only when the other process's window holds a single commit.
- **iOS**: the system terminates a suspended app that holds a file lock in an App Group container, and the open lock is held while a database is open.
- **Test this area harder than any other.** Concurrent reads and writes from several processes, with processes killed at random, are the phase 3 exit criterion.

### Encryption **[Decided]**

Specified in [design/file-format.md](design/file-format.md#encryption).

- **Page-level AEAD with XAES-256-GCM or XChaCha20-Poly1305** and a random 24-byte nonce per page write. The tag is the page's check, stored in the page and in its parent's pointer. A new file gets XAES-256-GCM on a processor with AES instructions and XChaCha20-Poly1305 elsewhere (`crypto/page.rs`, `preferred_cipher`), because each is several times faster than the other on the processors it suits. The caller does not choose (decided 2026-09-29): either cipher opens a file on any processor, only more slowly on one it does not suit. The key block is always wrapped with XChaCha20-Poly1305.
- **A data key wrapped by a key-encryption key**, stored in every commit record. Every commit copies the key block of the commit before it. Changing a password is a sync commit that rewraps the key, followed by empty sync commits until no slot holds the old key block.
- **A password becomes a key through Argon2id**, with its parameters stored in the key block: 19 MiB, 2 iterations and 1 lane by default, which fits a mobile app extension's memory. Unauthenticated modes such as CBC are not used at all.
- **Commit records are authenticated too.** Page 0 is plain, so each record of an encrypted file carries a keyed BLAKE2b MAC under a key derived from the data key, and recovery refuses a record whose MAC fails. Without it, anyone who can write the file could assemble a record from existing pages. Any code that reads a record from disk, such as a process that finds another process's commit in phase 3, has to check the MAC.
- **The pager encrypts and decrypts.** `storage/pager.rs` seals every page it writes and opens every page it reads, so the layers above see plaintext and never know which kind of file they are in. The commit's tree pages are sealed in `btree/finish.rs` through the pager, since a parent records its children's tags.
- **Rust API**: `OpenOptions::key`, `OpenOptions::password`, `OpenOptions::password_hashing`, `Database::set_key`, `Database::set_password`, `Database::is_encrypted`, and the errors `KEY_REQUIRED` and `WRONG_KEY`. Another handle to a file already open in the process has to present the key too.
- **The bindings take a key or a password as the Rust API does** (decided 2026-09-29 for Node.js): options to open with either, and calls to change them. In Node.js, `key`, `password` and `passwordHashing` in the options of `open`, `openAsync` and `salvage`, `isEncrypted`, and `setKey` and `setPassword` with their `Async` twins. The package copies the secret into buffers of its own and fills them with zeros once the engine has its copy, which it takes when the call is made, before an asynchronous task runs.
- **Operating-system keystores** (Keychain, Android Keystore, DPAPI) are worth offering as helpers in the bindings.
- **Encryption and multi-process access do not conflict here**, because there is no shared memory and no mmap.
- **The header stays plain**, so page size, transaction ids and file size are visible; everything inside a page is not.

### API and query model

The maintainer settled these on 2026-09-24. **[Decided]**

- **Queries have two forms that compile to one IR**: a builder in every language, typed where the language allows, and a string query language that the engine parses, so that every binding shares one parser. The IR is what the engine executes.
- **Schema migrations go by version.** The application declares a schema version. Opening a file whose schema is older applies the additive changes itself (new collections, new fields with a default, new indexes) and runs the application's migration function for each version step in between, all in one write transaction.
- **A primary key is a declared field or an auto-increment.** A collection names one field of type integer, string or bytes as its primary key, or gets a 64-bit integer that the engine assigns in increasing order.
- **Rust declares a schema with a builder at run time**, in the crate itself. A derive macro, which would be a separate proc-macro crate, is weighed in phase 5 together with Dart's code generation.
- **Node.js comes first with a synchronous API** (settled 2026-09-24): a schema declared in JavaScript with `t`, `collection` and `schema`, from which TypeScript infers every object's type; transactions scoped to a function, `db.read(fn)` and `db.write(fn)`, which commit when the function returns and abort when it throws; and a query builder typed with generics beside the query language. The asynchronous API on worker threads came next, with the same shapes: `openAsync`, `readAsync`, `writeAsync`, `syncAsync` and `closeAsync`, whose transaction functions may be asynchronous.
- **The Node.js package does the byte formats in JavaScript.** `lib/codec.ts` writes and reads records, the IR and the stored schema, so objects and queries cross the boundary as one buffer per call; the engine checks everything it is given. A migration's functions run in JavaScript through `OpenOptions::open_migrating`, which stops between version steps, rather than as callbacks from the engine.

The rest of this section is **[Tentative]**.

- **Query results are plain objects (snapshots).** No accessor objects tied to the database's lifetime, so there is no "used after the database was closed" crash to have.
- **v1 queries**
  - Primary keys.
  - Secondary indexes: single-field and unique. Composite indexes come in v2.
  - Comparisons: `== != < <= > >= between in`.
  - Strings: `contains`, `startsWith`, `endsWith`.
  - Null checks, `AND`, `OR`, `NOT`.
  - `sort`, `limit` / `offset`, `count`.
  - Links (to-one and to-many), backlinks, embedded objects, lists.
- **Language APIs**
  - Rust: the schema builder above, and a query builder in the crate itself.
  - Dart: schema and a type-safe query builder generated with `build_runner`.
  - TypeScript: a builder typed with generics.
- **The language boundary costs more than it looks.** Real-world performance is often decided by the binding layer rather than the engine, so records cross as packed binary buffers in one call, and batch APIs are the default.
- **Node.js**: a synchronous API, and an asynchronous one whose operations run on the libuv thread pool (napi-rs `AsyncTask`), so the event loop is never blocked. The pool was chosen over `worker_threads` because an operation there needs no second JavaScript realm and nothing copied into one: the native transaction moves to a pool thread behind a mutex, and the result comes back as one buffer.
- **Change detection**: reactive notifications are out of scope. If something is needed later, it is something small, such as reading a commit counter.

### Errors

- **Every failure has a stable code**, `Error::code` in `crates/darudb/src/error.rs`, in `SCREAMING_SNAKE_CASE`. Bindings pass it through unchanged (`error.code` in JavaScript). A code, once released, is not renamed; retiring one is a breaking change.
- **A database file is untrusted input.** Anything read from disk is validated before it is used, and a damaged file produces an error, never a panic.

### Bindings and distribution **[Tentative]**

- **Node.js**: napi-rs. Node-API is ABI-stable, so a binary does not need rebuilding per Node.js version. Prebuilt binaries ship as per-platform optional npm packages.
- **Dart**: `dart:ffi` with Dart build hooks (native assets), official from Dart 3.10 and Flutter 3.38, which are therefore the package's minimum versions (decided 2026-09-29). The Rust code is built with `native_toolchain_rust` or a similar package. The hook's imports belong in `dependencies`, not `dev_dependencies`, or the hook does not compile in consumer apps.
- **Toolchain**: the Rust version is pinned in `rust-toolchain.toml` for reproducible builds, which `native_toolchain_rust` requires. `rust-version` in the workspace `Cargo.toml` is a separate promise: the oldest compiler a crate consumer may use. It stays at 1.85, the first release with the 2024 edition, and rises only when a feature needs it, with a changelog entry (decided 2026-09-29).

### Platform baseline (reference)

|         | Rust Tier 1 minimum                                                         | Flutter minimum            |
| ------- | --------------------------------------------------------------------------- | -------------------------- |
| Windows | 10 (Windows 7 and 8 use the separate `*-win7-windows-msvc` targets, Tier 3) | 10 (from Flutter 3.19)     |
| macOS   | 10.12                                                                       | 10.14                      |
| iOS     | 10                                                                          | —                          |
| Linux   | kernel 3.2, glibc 2.17                                                      | —                          |
| Android | —                                                                           | API 21 (from Flutter 3.22) |

- Rust is not the bottleneck: Flutter's own minimums are higher.
- Windows 7 and 8 are not supported (decided 2026-09-29): their Rust targets are Tier 3, which would mean building and checking the standard library ourselves.
- Android needs 16 KB page support, including 16 KB alignment of the native `.so` files.

## Designs this project rejects

Each of these was tried elsewhere and caused the problems this project exists to avoid. Changing one of them needs a reason written down beside it.

| Rejected                                                | Why                                                                                     |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Live objects: accessors tied to the database's lifetime | Every access after a close is a crash waiting to happen, in every binding               |
| Mutexes in a shared-memory lock file                    | A process that dies holding one wedges the rest; the layout breaks across architectures |
| CBC with a separate MAC keyed from a weak password hash | Unauthenticated modes are fragile, and a fast-hash key falls to GPU brute force         |
| Shipping without recovery tools                         | A damaged file with no way to check or salvage it is lost data                          |
| A storage format entangled with sync                    | Sync is out of scope, and its needs would shape every page of the file                  |

## Things that surprise

- **The file format is not stable yet.** `format::FORMAT_VERSION` identifies it, and any change to what is on disk changes that number. Until the first release there are no migrations: a file from an older build is refused with `UNSUPPORTED_FORMAT_VERSION`, not upgraded. From the first release on, opening a file in an older format upgrades it, unless an option turns that off for an application that may roll back, which then upgrades it with an explicit call (decided 2026-09-29). The code implements `design/file-format.md` as far as phase 1 has reached; the module map above says which parts exist.
- **The storage kernel stores named trees of byte keys and byte values.** Keys are ordered as unsigned bytes and nothing else; typed objects are the object layer in `schema/`, built on top. Its trees have names that begin with a NUL character, which `tree_names` leaves out and the kernel's public calls refuse; the engine reaches them through the `*_in` methods of the transactions.
- **A bound prepared query still holds its parameters.** `Query::bind` keeps the values beside the prepared query's IR, which the two share through an `Arc`, rather than copying the IR with the values in place, so `Query::ir` of a bound query has `Expr::Prepared` in it. The planner reads each value from `Query::parameters` where the IR names a parameter; `bound_ir` makes the IR with the values in place, only for equality and for encoding.
- **Each handle keeps the schema it was opened with.** Handles to one file share an instance, but the schema lives on `Database`, and every transaction gets its handle's. Reaching a collection compares the stored schema's record with the handle's, which is how a handle notices another handle's or another process's migration (`SCHEMA_MISMATCH`).
- **The header in memory is not the file's.** Another process may commit at any moment, so a reader reads the header from the file and locks its snapshot's byte, and a writer reads it again under the writer lock (`Shared::refresh_header`). The instance's copy is what this process's writer last knew, and a difference from the file is how it learns that another process committed.
- **`sys/` is the only module with `unsafe` code.** The crate denies `unsafe_code`, and that module allows it for the operating system's calls the standard library does not offer: byte-range locks in `sys/lock.rs`, and in `sys/fs.rs` the rename that never replaces a file, for file systems without links, and what identifies a file on Windows. On Unix-like systems `sys/fs.rs` makes its calls through `rustix`, without `unsafe`. Clippy requires a `SAFETY` comment on every `unsafe` block, and one unsafe operation per block.
- **Nothing in a process may open a database file that the process has open**, tests included. On Unix-like systems, closing any descriptor of a file releases every lock the process holds on it. `tests/transactions.rs` reads the selector byte from another process for that reason.
- **Test builds have hooks in the engine.** `testing::pause_before_registering` stops a reader between its first read of the header and the registration of its snapshot, and `testing::pause_in_recovery` stops a process in the middle of recovery; `lock/tests.rs` uses them to test the second read and the recovery lock. `testing::pause_in_salvage` stops a salvage once it holds its file. `testing::KEEP_SNAPSHOT_LOCKS` turns off keeping snapshot locks, in half the workers of the multi-process suite.
- **Salvage holds a file without an instance.** It reads a file that may not open, so it takes the open lock alone through a handle of its own and marks the file in the registry with an entry that has a hold and no instance (`Entry::held`). Opening that file in the process fails with `BUSY` until salvage ends, rather than waiting as it waits for an instance that is closing, since that wait holds the registry and salvage can take minutes.
- **An open unsynced window has a thread.** `instance.rs` starts `darudb-sync` when a deferred commit opens a window with a time limit, and the thread ends when the window does. The same thread watches a commit another process left unsynced (`notice_unsynced`, from `begin_snapshot` and from opening a file another process has open), and ends that window under the writer lock if the commit is still published and unsynced a time limit later. The crash suite turns the time limit off, so its runs replay from their seed.
- **Kept snapshot locks have a thread.** A snapshot lock outlives its last reader by 20 milliseconds, for the next read transaction to join, and one `darudb-keeper` thread per process releases the kept ones; it ends after ten seconds with nothing to do.
- **Debug builds of the engine are optimized** (`opt-level = 2` in the root `Cargo.toml`), with debug assertions and overflow checks still on. The cipher's generic code is compiled into the engine, and unoptimized it made the crash suites forty times slower.
- **Half the crash suite's runs, and every other process-kill round, use an encrypted file.** A test that builds a database for the engine's internals should say which kind it uses.
- **A write transaction stores a collection's auto-increment counter only when it commits.** `WriteTransaction::insert_later` keeps a value until then, and a read by key sees it; walking or counting a tree with a value waiting is refused as an internal error, so a new use of `insert_later` must be for a tree the engine never walks. The engine's `\0meta` tree, which holds the counters, is read only by key.
- **A leaf a write transaction changes is its page** (`btree/leaf.rs`): entries are inserted and removed in the page's own layout, and a removed entry's cell is zeroed at once, so that no removed value stays on disk. The page is compacted only when an insert needs the room removed entries left, and is otherwise written with those zeroed gaps (decided 2026-09-27): compacting every such leaf before writing it took a third of a commit that deleted scattered objects. A branch it changes is decoded into its children and `Keys`, which holds the separator keys in one buffer rather than one allocation each. Both keep the heads of their keys (`Heads` in `btree/node.rs`), copied from the cached node, kept up to date as keys come and go, and handed to the node the commit caches, so that neither a search of a changed node nor the commit reads every key.
- **A write transaction takes the nodes of the unsynced window out of the page cache.** The first change to a committed node written after the durable commit takes it from the cache, with no copy, when nothing else holds it (`Load::node_to_change` in `btree/mod.rs`); an older node, or one a reader holds, is copied as before. Copying every node a small deferred commit changed took a tenth of it. A transaction dropped after taking a node leaves the cache without it, and the next reader of that page reads it from the file again.
- **A cached node keeps a head of every key beside its page** (`LoadedNode` in `btree/node.rs`): the four bytes after the prefix the node's keys share, so that a search reads a key only where two heads are equal. They are in memory only, and the page cache counts them against `cache_size`. Kept in the slots on disk, they made point lookups no faster than this and took a sixth of the entries an index leaf holds, which slowed walks over an index and writes.
- **The free runs outlive the write transaction.** The instance keeps the last commit's free runs in memory, tagged with its transaction id, and the next write transaction starts from them instead of reading the free tree. The commit rewrites only the runs that changed. The unit tests compare the kept runs with the free tree every time they are used.
- **Only the writer that made a commit knows the young part of its retained group.** A commit writes the pages it stopped using that were written after the durable commit in entries of their own, and the instance keeps those entries' runs in memory, tagged with the commit, like the free runs; the next write transaction reclaims them before the unsynced window ends, once no snapshot lies between the durable commit and the group (`design/commits-and-recovery.md`, "Pages written in the unsynced window"). A writer that finds another commit published reclaims those groups whole, after the window. The unit tests compare the kept parts with the retained tree every time they are used.
- **A writer that does not know which selector a power cut would bring back**, after opening the file or after another process committed, avoids the newest record older than the durable commit in its place (`possibly_trusted` in `txn/write.rs`), and issues an extra barrier only when that leaves no slot (`design/commits-and-recovery.md`, "Choosing the slot"; decided 2026-09-29). Before, it always issued one, which doubled the cost of sync commits that processes made in turn. `crash.rs` tests the rule with two instances on one simulated disk, `open_io` and `open_io_beside`.
- **`packages/node/native.js` and `native.d.ts` are generated** by `npm run build` from the `#[napi]` items in `src/lib.rs`, together with the `.node` addon, and all three are git-ignored. They are the package's internals. `lib/`, in TypeScript, is written by hand and is the API: `lib/types.ts` declares it, and `lib/index.ts` gives those types to what the other modules make, with one cast each, since the types know an object's fields by its schema and the code does not. Types in `lib/index.ts` check that the code has every member the declared types promise, and fail the build naming any it lacks; a change to the native layer that a user can see changes `lib/types.ts` too. `tsc` compiles `lib/` into `dist/` (git-ignored, never minified), declarations included, which is what the package ships and the tests load. Modules are loaded with `import x = require(...)` where a name would otherwise become a getter: a namespace import of `native.js` puts one on every call into the engine; `dist/` is as deep as `lib/`, so `../native.js` names the loader from either. `npm test` compiles `types/check.ts` against `dist/index.d.ts`, where a line marked `@ts-expect-error` that stops being an error fails the run.
- **The Node.js asynchronous API cannot wait on the thread pool for this process's own writer.** The pool has four threads by default, and a write transaction waiting there for another one in this process could take every thread while the one it waits for needs a thread to finish. So `lib/async.ts` queues this process's writes on each file in JavaScript, keyed the way the engine tells files apart (device and inode, or the real path on Windows), and hands them to the pool one at a time; `syncAsync` and `closeAsync` queue with them, since a sync waits for the writer while a deferred commit is not durable. A synchronous `write`, `sync` or `close` while an asynchronous write holds the file, or any of them from inside a write's function, would wait for a writer that needs the event loop, and is refused with `INVALID_ARGUMENT`. A migration holds the file the same way while its functions run.
- **An asynchronous operation that fails resolves rather than rejects on the native side.** A napi-rs task can reject only with a fixed set of status codes, so a failure resolves to `{ code, message }` and `settle` in `lib/async.ts` throws it as the engine's error. Operations of one transaction run in the order they were called: `Serial` sends those called in one turn, or while a batch is on the pool, as the next batch through `runAsync`, one batch at a time, which is also what keeps the native transaction's mutex uncontended. A batch's results come back in one buffer of tagged values (`Batch` in `src/lib.rs`), since a JavaScript value per result would cost several Node-API calls each. `readAsync` begins its read on the main thread, which waits for no writer.
- **The Node.js package makes each layout's objects with code it generates** (`builderOf` in `lib/codec.ts`): one object literal per layout, since assigning fields one by one under their names was most of what decoding a record cost. Field names come from the file, so they go into the code only through `JSON.stringify`, and nothing else from the file goes into it at all. A layout with a field named `__proto__`, which a literal would take for the prototype, and a process that forbids making code from strings fall back to assigning the fields one by one. Records are written the same way (`encoderOf`): code made for each layout reads each field under its own name and writes a scalar where it reads it, and falls back to walking the layout for the same layouts and for one of 128 fields or more. A top-level record is read the same way too (`decoderOf`), each field in the `switch` case of its id; an embedded object is read field by field.
- **The Node.js package reads and writes objects through functions given a handle, not through methods.** A napi-rs method call unwraps its object and registers the borrow in a map behind a lock, and so does a class object passed as an argument, about 80 nanoseconds more than a function given an `External`. So `getRecord`, `find`, `writeRecord` and the other calls a transaction makes most are functions of `native.js` that take the transaction's handle (and `NativePrepared.handle`), and the collection's name as a handle too, made once per layout by `nameOf` in `lib/shared.ts`, since a string argument is converted on every call. A synchronous `read` or `write` begins with `beginReadHandle` or `beginWriteHandle`, which give the handle alone, and ends with `commitTransaction` or `endTransaction`: a transaction object and its handle, each with a finalizer, cost more than the rest of a read transaction begun for one lookup. The asynchronous API and migrations use `NativeTransaction` objects, and read their handle once.
- **A synchronous read in Node.js returns its bytes in a buffer the package reuses.** The native layer copies a record or a query's records into `scratch` in `lib/shared.ts` and returns their length, or a `Buffer` of their own when they do not fit, since a new `Buffer` for every read cost an allocation and a collection. A query's records are gathered first in a vector the thread keeps for its next query (`FOUND` in `src/lib.rs`), which holds on to up to 1 MiB after a large result; a larger one is handed over with the vector. What is in it has to be decoded before the next call into the native layer, and decoding copies out every string and byte value, so nothing keeps a view of it.
- **A synchronous write of a few objects encodes them in a buffer the package reuses.** `lendRecords` in `lib/codec.ts` writes up to 16 records into `recordWriter` and hands the engine that buffer whole, with the length written (`recordBytes`), since a buffer of their own for every `put` cost more than encoding the object, and a view made of the kept one for every write cost about 5% of an insert. `lendChanges` does the same for an update. A larger batch, an asynchronous write, and a write a getter makes while an object is being encoded (`recordsLent`) get a buffer of their own, and the reused one is made small again after a large record grew it.
- **The strings of a short record are cut from one text of the record.** `Reader.string` in `lib/codec.ts` makes a Latin-1 string of a record of at most 512 bytes (`SHARED_TEXT`) at its first ASCII string, and cuts that string and the record's others out of it, since the call that makes a string from bytes costs several times what a cut does. A string cut out may keep the whole text alive, so a string of a record can hold up to 512 bytes; a longer record makes each string on its own.
- **The Node.js addon allocates through mimalloc** (version 2, with local dynamic TLS; `packages/node/Cargo.toml` says why), while the engine crate uses whatever allocator the program it is built into has. The system allocator of macOS zeroes what it frees, which took about a tenth of a lookup through the package. Building the addon compiles mimalloc's C source, so it needs a C compiler for every target.
- **The Electron test is a package of its own**, `packages/node/electron`, which runs the package in Electron's main process against `dist/` and the addon beside it. It sits outside `test/`, since `node --test` runs every script under a `test` folder, and it installs on its own, since Electron is large. npm may skip the install script that unpacks Electron's binary; `node node_modules/electron/install.js` there does it.
- **`npm test` in `packages/node` runs against the addon that is already built.** A change to the engine does not reach the Node.js tests until `npm run build` runs again.
- **The workflows run only when started by hand** (`workflow_dispatch`), because the account's GitHub Actions minutes are short for now. Local runs are the normal check. Each workflow file says where the `push` and `pull_request` triggers go when that changes.
- **A release publishes nothing unless asked.** `.github/workflows/release.yml` builds, tests and lists one package, and publishes it only with its `publish` input, after the package's changelog names the version. It publishes with the tokens `NPM_TOKEN` and `CARGO_REGISTRY_TOKEN`, then tags the commit `darudb-v<version>` or `node-v<version>` and makes a GitHub release from the changelog's section (decided 2026-09-29). The first release waits for the maintainer. [CONTRIBUTING.md](CONTRIBUTING.md#releasing-a-package) has the procedure. Each npm platform package ships `LICENSE` and the notices `packages/node/scripts/notices.mjs` writes, and `LICENSE` in `crates/darudb` and `packages/node` is a copy of the one at the root, since a package ships only files inside its own folder.
- **`docs/*/changelog.md` is generated** from the packages' `CHANGELOG.md` files and git-ignored. Edit the package's changelog, never the page.

## The documentation site

- **Two locales, `docs/en` and `docs/ko`, that mirror each other page for page.** A page added to one is added to the other. Korean is written, not translated from the English, and the heading anchors in Korean pages are the Korean words.
- **The sidebar is generated from the folder tree** by `vitepress-sidebar` and then reshaped in `.vitepress/config.ts`. Frontmatter `title` names a page and `order` places it.
- **A page's `<meta name="description">` is its own first paragraph**, read out of the source. Open every page with one sentence that says what it is about.
- **`robots.txt` and `llms.txt` are written at build time**, in `buildEnd`, from `packages/node/package.json`'s homepage and the pages that are actually there. Neither is committed.
- **Documentation is written with the feature, not before it.** A page describes what the code does today. A goal the code has not reached yet is labelled as a goal.

## Conventions

- **The specifications change with the code.** A change to what is written to disk, or to how commits, recovery or locks work, updates the matching document in `design/` in the same commit.
- **Formatters and linters are gates.** `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` at the root; `npm run lint` and `npx prettier . --check` in `packages/node` and `docs`.
- **Commits are `[scope] tag: message` in English**, with identifiers in backticks and one logical change each. The scopes are `[core]`, `[node]`, `[docs]` and `[common]`; the tags are in [CONTRIBUTING.md](CONTRIBUTING.md#write-a-commit-message).
- **A library change gets a changelog entry** under `## vNext` in the package it landed in, unless nothing a consumer can see changed. The packages version independently.
- **The two locales are updated together.** An API added and not documented in Korean is an unfinished change.
- **Tests write only into temporary directories**, one per test, never into a fixed path.

## References

- Dart build hooks: https://dart.dev/tools/hooks
- native_toolchain_rust: https://github.com/GregoryConrad/native_toolchain_rust
- napi-rs: https://napi.rs/
  - Support and compatibility: https://napi.rs/docs/more/support-compatibility
- Rust platform support: https://doc.rust-lang.org/rustc/platform-support.html
