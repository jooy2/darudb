# Locking

Status: accepted.

How any number of processes share one database file. The only state they share is the file's content and the operating system's byte-range locks on it: no shared memory, no lock file with a layout in it, and nothing that a process dying can leave behind for the others to clean up. [Commits and recovery](commits-and-recovery.md) says what the writer does once it holds the lock; this document says how it gets there and how readers stay out of its way.

## Goals

- Any number of processes open the file. One of them writes at a time; all of them can read at any time.
- Readers never wait for the writer, and the writer never waits for readers.
- A process that dies, at any moment and for any reason, leaves nothing that another process has to clean up or wait out.
- Every platform behaves the same way, so that one test suite covers all of them.

## One file handle per process

On Unix-like systems, a record lock belongs to the process, not to the file descriptor it was taken through. Two consequences shape the design. Closing **any** descriptor of the file releases **all** of the process's locks on it. And a process never conflicts with its own locks, so it cannot see them by asking the operating system.

The engine therefore keeps, per process, one shared instance for each database file. The instance holds the only operating-system handle to the file, the registry of the process's snapshots, the process's writer mutex and the page cache. Every `Database` object for that file in that process uses the same instance, and the last one to close closes the handle.

- **Files are identified by what they are, not by their path**: device and inode on Unix-like systems, volume serial number and file index on Windows. Two paths to the same file, through a link or a different spelling, share one instance.
- **Nothing else in the process may open the database file**, not even to copy it. Closing that descriptor would silently drop the engine's locks. The engine's own tools use the shared handle.
- **A child process does not inherit record locks.** An instance records the id of the process that opened it, and in a forked child every inherited `Database` object behaves as closed, failing with `CLOSED`. The child opens the file itself.

On Windows, a lock belongs to the handle. With one handle per process, the behaviour is the same on every platform.

## The lock bytes

All locks are taken on bytes far past the end of any data:

| Byte                  | Name     | Held                                                                        |
| --------------------- | -------- | --------------------------------------------------------------------------- |
| 2^62                  | Open     | Shared by every process that has the file open; exclusive during recovery   |
| 2^62 + 1              | Writer   | Exclusive by the process whose transaction is writing                       |
| 2^62 + 2              | Recovery | Exclusive by a process that is opening the file, until it has the open lock |
| 2^62 + 3              | Turn     | Exclusive by the waiting writer whose turn is next                          |
| 2^62 + 4 to 2^62 + 63 | Reserved |                                                                             |
| 2^62 + 64 + `s`       | Snapshot | Shared by every process with a read transaction on snapshot `s`             |

**Why so far from the data.** On Windows, byte-range locks are mandatory: a range locked through one handle cannot be read or written through another. Lock bytes that overlapped data would make that data unreadable. The file never reaches 2^62 bytes, so no read or write ever touches a lock byte. Both POSIX record locks and `LockFileEx` accept ranges past the end of the file.

**Offsets are 64-bit everywhere.** On 32-bit Unix-like targets, such as armv7 and i686, the engine uses the 64-bit variants of the lock calls.

**Transaction ids stay below 2^62 − 64**, so that every snapshot byte fits in a signed 64-bit offset. At a million commits a second, that lasts more than 100,000 years. A record with a larger one is not valid ([File format](file-format.md#commit-slots)), and a writer whose next transaction id would reach the limit fails with `CORRUPTED`, since only a damaged or forged file can get there.

## Opening and closing

To open the file, a process:

1. Opens the file for reading and writing, or finds its existing instance for it and stops here.
1. Takes the recovery lock exclusively. The attempt is repeated with increasing pauses, and waiting past the busy timeout fails with `BUSY`.
1. Tries to take the open lock exclusively, without waiting.
1. If it gets it, no other process has the file open. It runs [recovery](commits-and-recovery.md#recovery), then converts the open lock to shared.
1. If it does not, other processes have the file open, and the first of them recovered it. It takes the open lock in shared mode.
1. Releases the recovery lock.

A process holds the open lock only once it has recovered the file or found others holding it, never while it waits to open. That is what the recovery lock is for. If a process dies while it recovers the file, after a power cut, its open lock and its recovery lock go together, and the next process to take the recovery lock finds the open lock free and recovers the file itself. Were processes waiting for the open lock in shared mode instead, they would be granted it by the death and use the file unrecovered.

Converting the open lock happens under the recovery lock, so no other process can open the file in between: POSIX record locks convert in place, and on Windows the process takes the shared lock while still holding the exclusive one, then releases the exclusive one.

When the last `Database` object for the file in a process closes, the instance ends the process's unsynced window if it made deferred commits, releases its locks, and closes the handle.

**Why a lock held for as long as the file is open.** It is the only way for an opening process to know that no other process has the file open, and that is what decides whether a crash may have left something to recover. Its one cost is on iOS, described under [Platform notes](#platform-notes).

## Beginning a read

A reader takes no lock on the header. Instead it reads, registers, and reads again:

1. Read bytes 0 to 2047 of the file: the static fields, the selector and the three slots.
1. Take the record in the slot the selector names. If its check fails, read again: the reader may be holding a stale selector that points at a slot the writer is filling. If the check still fails after a few attempts, the header is damaged: `CORRUPTED`.
1. Register the snapshot `s`, the record's transaction id. The first registration of `s` in the process takes a shared lock on `s`'s snapshot byte; later ones only count up in the process's registry.
1. Read the selector and the published record's transaction id again. If they are unchanged, the snapshot is safe to use. Otherwise, unregister `s` and start over.

**Why this is safe.** A page that snapshot `s` can reach is overwritten only after some commit `F > s` stops using it and a writer reclaims group `F`. A writer reclaims `F` only in a transaction that starts after `F` was published, and it looks for registered snapshots when it starts. Step 4 saw `s` still published, so `F` was published after step 4, which came after the lock of step 3. The writer's search therefore finds that lock.

**What it costs.** Two reads of the first 2048 bytes of the file, and one lock call, which is skipped when another transaction in the process already holds `s`. A reader never waits for a write transaction; at most, on Windows, it waits an instant while a writer probes the snapshot bytes.

## Ending a read

The reader unregisters its snapshot. When the process's count for `s` reaches zero, the lock on `s`'s snapshot byte is released.

A read transaction that is never ended keeps every page it can reach from being reused, so the file grows while it lives. A binding should make leaking one hard: by closing it when its object is collected, and by warning about read transactions that stay open for a long time.

## Writing

1. Take the process's writer mutex, so that one thread at a time competes for the file.
1. Take the writer lock exclusively. The attempt does not block: it is repeated with increasing pauses, no longer than a millisecond, until it succeeds or the busy timeout passes, which fails with `BUSY`. Polling rather than blocking is what makes a timeout possible on every platform. Each attempt goes as follows:
   1. If another process holds the turn lock, leave the writer lock alone.
   1. Otherwise try the writer lock, without waiting.
   1. If that failed and the writer has waited 50 milliseconds, try the turn lock, without waiting. A writer that gets it keeps it until it has the writer lock, and its pauses grow no longer than 100 microseconds.
1. Hold both until the commit or the abort has returned.

While it holds the writer lock, the writer starts its transaction and commits it as [Commits and recovery](commits-and-recovery.md#starting-a-write-transaction) describes. It writes records and the selector without any further lock, since readers verify everything they read.

**No deadlock is possible.** The writer waits for nothing while it holds the writer lock, apart from its own barriers. A writer holding the turn lock waits only for the writer lock, and the holder of that waits for nothing. Readers never wait for either. The recovery lock and the open lock are taken, in that order, only while a process opens the file, before it has any transaction, and the open lock is held exclusively only during recovery.

**Why a turn lock.** Record locks do not queue waiting processes, and a writer that commits again as soon as it has finished takes the writer lock back before a pausing one wakes. The phase 3 tests saw that happen: with four processes committing in tight loops, most writers waited microseconds, but some waited seconds, past the default busy timeout. The turn lock bounds the wait. A writer that has waited 50 milliseconds claims the next turn, and every other writer, the one that just finished included, lets it go first. Among several writers claiming the turn, each tries as often as the others, so none is favoured.

**Why 50 milliseconds.** Each time the writer lock passes from one process to another, the next commit issues a barrier before its record ([Commits and recovery](commits-and-recovery.md#choosing-the-slot)), so passing the lock on every commit would make contended writers pay a barrier each. Claiming the turn after 5 milliseconds did that, and two processes committing in tight loops made a twentieth of their commits. After 50, they made as many as without a turn, and the longest wait was about 60 milliseconds.

## Finding the oldest snapshot

When it starts, the writer has to know which retained groups no snapshot can still reach ([Commits and recovery](commits-and-recovery.md#reclaiming-pages)). It asks only that question, and in as few calls as it can:

1. Among the retained groups that the durable commit allows (group ids up to `D`), take the largest, `F`.
1. Ask whether any snapshot below `F` is registered, in this process or any other. If none is, every one of those groups is reclaimable, and one query was enough.
1. Otherwise, bisect over the group ids with the same question, to find the largest group that no snapshot can reach.

Each question is answered in two parts:

- **This process**: the in-process registry of snapshots.
- **Other processes**, on Unix-like systems: `fcntl(F_GETLK)`, asking for an exclusive lock over the range of snapshot bytes below `F`. It reports whether a conflicting lock exists. It does not report the lowest one, which is why the search bisects instead of asking for it.
- **Other processes**, on Windows, which has no such query: try to take an exclusive lock over the same range without waiting, and release it at once if granted. Granted means no other lock lies in the range. The probe also collides with this process's own snapshot locks, which is harmless: those snapshots count anyway.

A Windows probe can make a reader's snapshot lock wait for an instant. The reader waits for it, and the writer never waits while it holds a probe, so the two cannot deadlock.

## The page cache

Each process caches the pages it reads, keyed by page number **and** check. A reader always knows the check it expects, from the pointer that led it to the page. So a cached copy of an older page at the same number never matches, and it is never used.

Nothing is invalidated when another process publishes a commit: entries that no longer match anything simply age out. That keeps the cache useful when several processes write in turn, where dropping it on every new commit would empty it each time another process committed. In an encrypted file the check is the tag, and every write of a page draws a fresh nonce, so two versions of a page never share a key.

## When a process dies

The operating system releases every lock the process held, and that is all the cleanup there is:

- **Its snapshots** stop holding pages back. The next writer reclaims them.
- **The writer lock** passes to the next writer, which starts from the published commit. Whatever the dead writer had written sits at free positions or in a slot that no one reads, and the next writer's choice of slot takes care of it ([Commits and recovery](commits-and-recovery.md#choosing-the-slot)).
- **The open lock**: if it was the last process with the file open, the next process to open it runs recovery.
- **The recovery lock**, held while it opened or recovered the file: the next process waiting for it takes it and finds the open lock as the dead one left it, free if the dead one was recovering.
- **The turn lock**: the other writers stop waiting for it and compete for the writer lock again.

## Platform notes

- **Linux and Android.** `fcntl` record locks, through the 64-bit calls on 32-bit targets. An app's private storage supports them. Shared storage served through FUSE may not; when the lock call reports that locks are unsupported, the open fails with `UNSUPPORTED_FILE_SYSTEM`.
- **macOS and iOS.** `fcntl` record locks, and `F_FULLFSYNC` as the barrier, which Rust's standard library already uses for `sync_all`. On iOS, the system terminates a suspended app that holds a file lock inside an App Group container. The open lock is held for as long as the database is open, so an app whose database lives in an App Group container has to close it before the app is suspended. A database in the app's own container is not affected.
- **Windows.** `LockFileEx` and `UnlockFileEx`. Locks are mandatory, which is why the lock bytes sit far from the data. An unlock has to name exactly the range that was locked. A handle may hold a shared and an exclusive lock on the same range, and the first unlock releases the exclusive one, which is what converting the open lock relies on. Should Windows refuse the shared lock alongside the exclusive one, the process releases the exclusive lock first and then takes the shared one; the recovery lock it still holds keeps every other process out of the gap. The barrier is `FlushFileBuffers`; there is no way to sync a directory, and none is needed on NTFS, which journals the change itself.
- **Network file systems** are detected and refused with `UNSUPPORTED_FILE_SYSTEM`: by `statfs` on Linux (NFS, SMB and the like), by the file system name on macOS (`nfs`, `smbfs`, `afpfs`, `webdav`), and by `GetDriveTypeW` and UNC paths on Windows. Detection is best effort; a network file system that is not detected is still not supported.
- **Containers.** Processes in containers that share a local volume share the kernel, and so its locks, and work as usual. Hosts that share a volume over a network do not.

## Errors

This document adds two error codes to the engine:

| Code                      | When                                                                                                                             |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `BUSY`                    | The writer lock, or the recovery lock while another process opens or recovers the file, was not granted within the busy timeout. |
| `UNSUPPORTED_FILE_SYSTEM` | The file is on a network file system, or on a file system whose locks do not work.                                               |

## What the phase 3 tests must show

These are the phase 3 exit criterion, "fuzzing that mixes concurrent reads and writes from several processes with forced kills passes", made specific.

- **Many processes, one file.** Processes open the same file and run random read transactions and random sync and deferred write transactions. Random ones are killed, with `SIGKILL` or `TerminateProcess`, and new ones start.
- **Every snapshot is one commit.** Each commit keeps an invariant across its entries, such as a fixed sum, and every reader verifies it. No page a reader reads may fail its check: a failure there means a page was reused under a live snapshot.
- **Nothing is lost.** After every process has stopped, the integrity check passes and every commit that returned is present, deferred ones included, since no power was cut.
- **One process, many handles.** Several threads and several `Database` objects for one file share one instance, and closing one never releases another's locks.
- **Every platform.** The suite runs on Linux, macOS and Windows in CI, since the lock semantics are where the platforms differ most.
