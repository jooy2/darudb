# Working in this repository

What a reader has to know before changing anything here, and where to find the rest. [CONTRIBUTING.md](CONTRIBUTING.md) has the full procedure and every command; this file is the map, the requirements and the design decisions. [TODO.md](TODO.md) is the roadmap, the open questions and the work that outlived a session. [design/](design/README.md) holds the specifications of the file format, the commit and recovery protocol, and the locking protocol.

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
| `format/`     | What bytes on disk mean: every layout of `design/file-format.md`. No I/O at all          | Phase 2          |
| `lock/`       | Cross-process coordination through file range locks                                      | Phase 3          |
| `crypto/`     | Page encryption, key wrapping, key derivation. No I/O, like `format`                     | Phase 2          |
| `schema/`     | Collections, fields, indexes, schema migrations                                          | Planned, phase 4 |
| `query/`      | The query IR and its execution                                                           | Planned, phase 4 |
| `tools/`      | Integrity check, salvage, backup, compact                                                | Planned, phase 6 |

From the bottom up: `format` and `crypto`, then `storage`, `btree`, `space`, `lock`, `instance`, `txn`, `schema` and `query`, `tools`, and `database` on top. `lib.rs` re-exports the public surface and nothing below `database`'s level leaks into it.

Tests sit beside what they test, plus three places that test the whole engine:

- `src/crash.rs`: the crash suite. Random transactions on the simulated disk of `storage/sim.rs`, cut by power failures and process deaths, then reopened and compared with the history of commits, with an integrity check of every page. Half the runs use an encrypted file. `DARUDB_CRASH_SEEDS` makes it longer.
- `tests/process_kill.rs`: real child processes killed while they commit. `DARUDB_KILL_ROUNDS` makes it longer.
- `tests/transactions.rs` and `tests/open.rs`: the public API on real files.

`examples/kernel_bench.rs` measures the storage kernel: commits, bulk writes, reads and large values, on a plain file and on an encrypted one, and opening a file with a password. It is for comparing two builds on one machine, and a performance change quotes its numbers from before and after.

## Scope **[Decided]**

- **Targets**
  - Rust: the crate is a public API, not only the engine behind the bindings.
  - Node.js: servers and desktop applications.
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
- **No mmap; positional reads and writes (`pread` / `pwrite`) through our own page cache.** Memory-mapped files are hard to prove sound in Rust, because another process can change the mapped bytes under a live reference, and they conflict with both multi-process access and encryption. The cost is the zero-copy read path mmap would give, so the performance goal has to be proven by benchmarks against this design.
- **Page size**: a power of two from 4096 to 65536, recorded in the header; 4096 by default until the benchmarks decide. It is independent of the operating system's page size, which is never assumed (Android now uses 16 KB pages).
- **Every page reserves a 24-byte nonce field, encrypted or not**, so a tree has the same shape either way and one test suite covers both.
- **Storage layout** (**[Tentative]**, phase 4): a record tree `(collection, primary key) → record bytes`, and an index tree `(collection, field, value, primary key)`.

### Crash safety and recovery **[Decided]**

Specified in [design/commits-and-recovery.md](design/commits-and-recovery.md).

- **Five platform assumptions and no others**: a one-byte write is atomic, a write changes only the bytes it names, a successful sync makes earlier writes durable, byte-range locks work and die with their owner, and the file system is local. [design/README.md](design/README.md#what-the-engine-assumes-of-the-platform) states them exactly.
- **A sync commit costs one barrier.** The barrier is the commit point; flipping the selector afterwards publishes a commit that is already durable, so readers never see one that a power cut could undo.
- **Deferred commits** are published without a barrier and become durable at the next one: the next sync commit, `Database::sync`, closing the database, or the window's limits on pages and time. A power cut undoes them only from the newest backwards; a process crash loses none.
- **Three records protected at every commit**: the published one, the durable one, and the one a power cut would make recovery trust without checking. When no slot is left, the commit issues a barrier first.
- **Recovery** runs in the first process to open the file. It adopts the newest commit that is either published with the unsynced bit clear or passes checking, where checking reads only the pages written since that commit's durable transaction id.
- **A failed barrier is not retried.** The commit fails with `SYNC_FAILED`, and the handle is unusable until the file is reopened.
- **A new database is written to a temporary file and moved into place without replacing anything**, so the path holds either nothing or a complete database.
- **Tools to ship with the library**: an integrity check, salvage (build a new file from the pages whose checks are valid), online backup, and compaction.
- **Not supported**: network file systems (NFS, SMB). They are detected and refused with `UNSUPPORTED_FILE_SYSTEM`.

### Several processes **[Decided]**

Specified in [design/locking.md](design/locking.md).

- **No mutexes in shared memory.** A process that dies holding a shared-memory mutex leaves it held, recovering from that needs robust mutexes that not every platform has, and a lock file with a memory layout in it breaks between processes of different architectures.
- **Only operating-system byte-range locks** (`fcntl` on Unix, `LockFileEx` on Windows), on bytes from 2^62 up, where no data ever is: an open lock, a writer lock, and one byte per snapshot. When a process dies, the operating system releases its locks, and nothing else needs cleaning up.
- **One operating-system handle per file per process**, shared by every `Database` object for that file, because closing any descriptor drops all of a process's POSIX locks.
- **Readers take no header lock**: they read the header, register their snapshot, and read it again.
- **The writer reclaims pages** only from groups that no registered snapshot and no possible recovery can still reach.
- **The page cache is keyed by page number and check**, so a stale entry never matches and nothing has to be invalidated when another process commits.
- **Concurrency model**: one writing process at a time and any number of readers. Waiting for the writer lock past the busy timeout fails with `BUSY`.
- **iOS**: the system terminates a suspended app that holds a file lock in an App Group container, and the open lock is held while a database is open.
- **Test this area harder than any other.** Concurrent reads and writes from several processes, with processes killed at random, are the phase 3 exit criterion.

### Encryption **[Decided]**

Specified in [design/file-format.md](design/file-format.md#encryption).

- **Page-level AEAD with XAES-256-GCM or XChaCha20-Poly1305** and a random 24-byte nonce per page write. The tag is the page's check, stored in the page and in its parent's pointer. A new file gets XAES-256-GCM on a processor with AES instructions and XChaCha20-Poly1305 elsewhere (`crypto/page.rs`, `preferred_cipher`), because each is several times faster than the other on the processors it suits. The key block is always wrapped with XChaCha20-Poly1305.
- **A data key wrapped by a key-encryption key**, stored in every commit record. Every commit copies the key block of the commit before it. Changing a password is a sync commit that rewraps the key, followed by empty sync commits until no slot holds the old key block.
- **A password becomes a key through Argon2id**, with its parameters stored in the key block: 19 MiB, 2 iterations and 1 lane by default, which fits a mobile app extension's memory. Unauthenticated modes such as CBC are not used at all.
- **Commit records are authenticated too.** Page 0 is plain, so each record of an encrypted file carries a keyed BLAKE2b MAC under a key derived from the data key, and recovery refuses a record whose MAC fails. Without it, anyone who can write the file could assemble a record from existing pages. Any code that reads a record from disk, such as a process that finds another process's commit in phase 3, has to check the MAC.
- **The pager encrypts and decrypts.** `storage/pager.rs` seals every page it writes and opens every page it reads, so the layers above see plaintext and never know which kind of file they are in. The commit's tree pages are sealed in `btree/finish.rs` through the pager, since a parent records its children's tags.
- **Rust API**: `OpenOptions::key`, `OpenOptions::password`, `OpenOptions::password_hashing`, `Database::set_key`, `Database::set_password`, `Database::is_encrypted`, and the errors `KEY_REQUIRED` and `WRONG_KEY`. Another handle to a file already open in the process has to present the key too.
- **Operating-system keystores** (Keychain, Android Keystore, DPAPI) are worth offering as helpers in the bindings.
- **Encryption and multi-process access do not conflict here**, because there is no shared memory and no mmap.
- **The header stays plain**, so page size, transaction ids and file size are visible; everything inside a page is not.

### API and query model **[Tentative]**

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
  - Rust: typed schema and query builder in the crate itself.
  - Dart: schema and a type-safe query builder generated with `build_runner`.
  - TypeScript: a builder typed with generics.
- **The language boundary costs more than it looks.** Real-world performance is often decided by the binding layer rather than the engine, so records cross as packed binary buffers in one call, and batch APIs are the default.
- **Node.js**: a synchronous API, and an asynchronous one that runs on worker threads so the event loop is never blocked.
- **Change detection**: reactive notifications are out of scope. If something is needed later, it is something small, such as reading a commit counter.

### Errors

- **Every failure has a stable code**, `Error::code` in `crates/darudb/src/error.rs`, in `SCREAMING_SNAKE_CASE`. Bindings pass it through unchanged (`error.code` in JavaScript). A code, once released, is not renamed; retiring one is a breaking change.
- **A database file is untrusted input.** Anything read from disk is validated before it is used, and a damaged file produces an error, never a panic.

### Bindings and distribution **[Tentative]**

- **Node.js**: napi-rs. Node-API is ABI-stable, so a binary does not need rebuilding per Node.js version. Prebuilt binaries ship as per-platform optional npm packages.
- **Dart**: `dart:ffi` with Dart build hooks (native assets), official from Dart 3.10 and Flutter 3.38. The Rust code is built with `native_toolchain_rust` or a similar package. The hook's imports belong in `dependencies`, not `dev_dependencies`, or the hook does not compile in consumer apps.
- **Toolchain**: the Rust version is pinned in `rust-toolchain.toml` for reproducible builds, which `native_toolchain_rust` requires. `rust-version` in the workspace `Cargo.toml` is a separate promise: the oldest compiler a crate consumer may use.

### Platform baseline (reference)

|         | Rust Tier 1 minimum                                                         | Flutter minimum            |
| ------- | --------------------------------------------------------------------------- | -------------------------- |
| Windows | 10 (Windows 7 and 8 use the separate `*-win7-windows-msvc` targets, Tier 3) | 10 (from Flutter 3.19)     |
| macOS   | 10.12                                                                       | 10.14                      |
| iOS     | 10                                                                          | —                          |
| Linux   | kernel 3.2, glibc 2.17                                                      | —                          |
| Android | —                                                                           | API 21 (from Flutter 3.22) |

- Rust is not the bottleneck: Flutter's own minimums are higher.
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

- **The file format is not stable yet.** `format::FORMAT_VERSION` identifies it, and any change to what is on disk changes that number. Until the first release there are no migrations: a file from an older build is refused with `UNSUPPORTED_FORMAT_VERSION`, not upgraded. The code implements `design/file-format.md` as far as phase 1 has reached; the module map above says which parts exist.
- **The storage kernel stores named trees of byte keys and byte values.** Keys are ordered as unsigned bytes and nothing else; typed keys, records and queries are the object layer of phase 4, built on top.
- **Only one process may have a file open until phase 3 is done.** The open lock of `design/locking.md` is held exclusively for as long as a file is open, so a second process waits for the busy timeout and fails with `BUSY`. Handles within one process share one instance and are safe together.
- **`lock/sys.rs` is the only module with `unsafe` code.** The crate denies `unsafe_code`, and that module allows it for the operating system's byte-range lock calls, which the standard library does not offer. Clippy requires a `SAFETY` comment on every `unsafe` block, and one unsafe operation per block.
- **Nothing in a process may open a database file that the process has open**, tests included. On Unix-like systems, closing any descriptor of a file releases every lock the process holds on it. `tests/transactions.rs` reads the selector byte from another process for that reason, and the lock tests run a second process of the test binary through `testing::Helper`.
- **An open unsynced window has a thread.** `instance.rs` starts `darudb-sync` when a deferred commit opens a window with a time limit, and the thread ends when the window does. The crash suite turns the time limit off, so its runs replay from their seed.
- **Debug builds of the engine are optimized** (`opt-level = 2` in the root `Cargo.toml`), with debug assertions and overflow checks still on. The cipher's generic code is compiled into the engine, and unoptimized it made the crash suites forty times slower.
- **Half the crash suite's runs, and every other process-kill round, use an encrypted file.** A test that builds a database for the engine's internals should say which kind it uses.
- **The free runs outlive the write transaction.** The instance keeps the last commit's free runs in memory, tagged with its transaction id, and the next write transaction starts from them instead of reading the free tree. The commit rewrites only the runs that changed. The unit tests compare the kept runs with the free tree every time they are used.
- **The first commit after opening a file issues one extra barrier**, unless recovery issued one: until then the writer does not know which selector a power cut would bring back (`design/commits-and-recovery.md`, "Choosing the slot").
- **`packages/node/index.js` and `index.d.ts` are generated** by `npm run build` from the `#[napi]` items in `src/lib.rs`, together with the `.node` addon, and all three are git-ignored. The TypeScript types a consumer sees are whatever the Rust source says.
- **`npm test` in `packages/node` runs against the addon that is already built.** A change to the engine does not reach the Node.js tests until `npm run build` runs again.
- **The workflows run only when started by hand** (`workflow_dispatch`), because the account's GitHub Actions minutes are short for now. Local runs are the normal check. Each workflow file says where the `push` and `pull_request` triggers go when that changes.
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
