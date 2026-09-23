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
| 1. Storage kernel       | Page file, copy-on-write B+tree, two commit slots with checksums; one process only     | No data lost across thousands of repeated `kill -9` and simulated power cuts                   |
| 2. Encryption           | Page AEAD, DEK and KEK, KDF                                                            | The same suite passes with encryption on and off, and the slowdown is measured                 |
| 3. Several processes    | The file range lock protocol, cache invalidation                                       | Fuzzing that mixes concurrent reads and writes from several processes with forced kills passes |
| 4. Objects and queries  | Schema, indexes, query IR, migrations                                                  | Benchmarks against established embedded databases at the same durability settings              |
| 5. Bindings and release | Dart build hooks, napi-rs, per-platform prebuilt binaries                              | A CI matrix that includes the oldest supported operating systems                               |
| 6. Tools                | Integrity check, salvage, backup, compaction                                           | Data recovered from deliberately damaged files                                                 |

The skeleton that exists today sits before phase 1: a Cargo workspace, the engine's module layout, a file header that is written and validated, a Node.js binding that opens and closes a database, test suites in both languages, and the documentation site.

## Open questions

- **Final minimum OS and runtime versions.** If Windows 7 and 8 are needed, the Tier 3 Rust targets have to be built by us.
- **JavaScript runtimes beyond Node.js**: Electron (whose main and renderer are themselves several processes), React Native, Bun, Deno.
- **Minimum Dart and Flutter versions.** Build hooks need Dart 3.10 or Flutter 3.38 at least.
- **Encryption**: the final cipher and KDF, and the shape of the key-management API.
- **Several processes in v1**: several writing processes, or one writer and many readers first?
- **Query API form**: string queries, a builder, or both.
- **Schema migrations**: how they are declared and when they run.
- **File format versioning**: the forward and backward compatibility policy, and whether an older file is upgraded on open or by an explicit call.
- **Performance goal**: the benchmark workloads, and the durability settings to compare at.
- **Default page size.** 4096 bytes today, which is a placeholder rather than a measured choice.
- **Minimum supported Rust version.** `rust-version` is 1.85 today, the first release with the 2024 edition. Whether to hold it there is open.
