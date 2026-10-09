# Design

The specifications the DaruDB engine is built to. Each document here is normative: where the code and a document disagree, one of them is wrong, and the change that fixes it updates both in the same commit.

| Document                                        | What it settles                                                                                          |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| [File format](file-format.md)                   | What every byte of a database file means                                                                 |
| [Commits and recovery](commits-and-recovery.md) | How a transaction changes the file, when a commit is durable, and what opening a file after a crash does |
| [Locking](locking.md)                           | How processes that share one file coordinate through operating-system file locks                         |
| [Objects and queries](objects.md)               | Collections, schemas, the key and record encodings, indexes, queries and migrations                      |
| [Tools](tools.md)                               | The integrity check, backup, compaction and salvage                                                      |

These documents are for the people and agents who build the engine. The documentation site explains the same behaviour to users, in English and Korean, once each part is implemented.

## Status

All three documents were accepted by the maintainer on 2026-09-23, at the end of phase 0 of the roadmap, and `CLAUDE.md` summarises them. They describe file format version 6 now. Version 3 added a MAC on each commit record of an encrypted file to version 2, the first that stores data, and the maintainer accepted it on 2026-09-24; version 4 writes the ints of object keys in the fewest bytes that hold them, and the maintainer accepted it on 2026-09-27. The same day the maintainer accepted that a writer may leave the zeroed bytes of the leaf entries it removed where they were, which needs no new version, since a reader of version 4 already reads such a leaf. Version 5 adds the unsynced window to each commit record, and is the format of the first release. On 2026-10-09 the maintainer chose version 6, whose leaf entries write their lengths as varints, and a migration that raises a file of version 5 in place, its leaves rewritten as writes change them.

[Objects and queries](objects.md), the specification of phase 4, was accepted by the maintainer on 2026-09-24. Phase 4 implements it.

[Tools](tools.md), the specification of phase 6, was written with the tools and accepted by the maintainer on 2026-09-29. It changes nothing on disk.

Phases 1 and 2 implement them for a single process: the file format, sync and deferred commits, recovery, and encryption. Phase 3 implements the lock protocol, on the same format: the open, writer and snapshot locks, several processes on one file, and refusing network file systems. `crates/darudb/src/processes.rs` holds the tests that [Locking](locking.md#what-the-phase-3-tests-must-show) asks for.

## What the engine assumes of the platform

The design is correct only on a platform that keeps these promises. Each document says where it relies on one.

1. **A one-byte write is atomic.** After a power cut, a byte that was being written holds either its old value or its new one.
1. **A write changes only the bytes it names.** Bytes outside the written range keep their values across a power cut, whether they share a sector with the write or not.
1. **A barrier is a barrier.** When a sync of the file returns successfully, every write issued to that file before the sync started is durable. The engine's barrier is `fsync` on Unix-like systems, `fcntl(F_FULLFSYNC)` on macOS and iOS, and `FlushFileBuffers` on Windows.
1. **Byte-range locks work, and die with their owner.** `fcntl` record locks on Unix-like systems and `LockFileEx` on Windows are honoured between processes, and the operating system releases a process's locks when the process ends for any reason.
1. **The file system is local.** Network file systems break promises 3 and 4, so the engine refuses to open a database on one.

Nothing else is assumed. Between two barriers, writes may reach the disk in any order. A write cut short by a power failure may leave any mix of old and new bytes inside its range. A page may be damaged while it sits on the disk. Checks on every page catch the last two.

## Terms

- **Page**: one of the equal-sized blocks the file is divided into. Page 0 is the header; every other page belongs to a tree or to an overflow run.
- **Check**: the 16 bytes that verify a page. In a plain file it is a hash of the page; in an encrypted file it is the page's authentication tag.
- **Pointer**: a reference from one page (or from the header) to another. It carries the page number, the commit that wrote that page, and the page's check.
- **Commit**: one completed write transaction, numbered by its **transaction id**. Transaction ids only grow.
- **Commit record**: the fixed-size description of one commit: its transaction id, the roots of its trees and its page count. The header holds three records, one in each **slot**.
- **Selector**: the byte in the header that says which slot holds the published commit, and whether that commit is known to be durable.
- **Published commit**: the commit the selector points at. A reader that starts now sees this commit.
- **Durable commit**: the newest commit guaranteed to survive a power cut.
- **Unsynced window**: the commits after the durable commit whose durability is not yet confirmed. It is empty unless deferred commits are in use.
- **Barrier**: a sync of the database file, as described in promise 3 above.
- **Snapshot**: the commit a read transaction sees for its whole life, named by that commit's transaction id.
- **Live, retained and free pages**: a page is live if the published commit can reach it, retained if only an older commit that may still be read can reach it, and free if nothing that may still be read can reach it.
- **Reclaim**: turn retained pages into free pages once no snapshot and no possible recovery can reach them.
- **Writer**: the one process, and the one transaction within it, allowed to write at a given moment.

## Changing a specification

- **A change to what is written to disk changes `FORMAT_VERSION`** in `crates/darudb/src/format/mod.rs`, and the version history at the end of [File format](file-format.md) gains a line. Version 5 is the format of the first release, version 1.0.0 of the npm and Dart packages, and every version change after it comes with a migration from the one before.
- **The code and the document change in the same commit.** The `format` module's tests pin the documented offsets, so a layout change that skips the document fails a test.
- **A new document is added when a new layer gets an on-disk shape.** The key encoding and the record layout of the object layer come with phase 4.
