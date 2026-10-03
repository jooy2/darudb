---
title: Locking
order: 5
---

# Locking

This page explains how several processes share one database file through the operating system's byte-range locks alone, with no shared memory and nothing for a dead process to leave behind.

The specification is [design/locking.md](https://github.com/jooy2/darudb/blob/main/design/locking.md) in the repository. [Several processes](../guide/processes.md) shows what it means for an application.

## What the locks promise

- Any number of processes open the file. One of them writes at a time, and all of them can read at any time.
- Readers never wait for the writer, and the writer never waits for readers.
- A process that dies, at any moment and for any reason, leaves nothing that another process has to clean up or wait out.
- Every platform behaves the same way, so one test suite covers all of them.

## Why only byte-range locks

The only state the processes share is the file's content and the operating system's byte-range locks on it: `fcntl` record locks on Unix-like systems and `LockFileEx` on Windows. The operating system releases a process's locks when the process ends, for any reason, and that is all the cleanup there is.

Coordinating through mutexes in shared memory was rejected. A process that dies holding such a mutex leaves it held, and recovering from that needs robust mutexes that not every platform has. A lock file with a memory layout in it also breaks between processes of different architectures. Designs built that way have in practice been limited to one process at a time, and making several processes on one file stable is one of the tasks DaruDB was started for.

## The lock bytes

Every lock is taken on a byte at 2^62 or above, far past any data:

| Byte | Lock | Held |
| --- | --- | --- |
| 2^62 | Open | Shared by every process that has the file open, and exclusively by a process recovering it |
| 2^62 + 1 | Writer | Exclusively, by the process whose transaction is writing |
| 2^62 + 2 | Recovery | Exclusively, by a process that is opening the file, until it holds the open lock |
| 2^62 + 3 | Turn | Exclusively, by the waiting writer whose turn is next |
| 2^62 + 64 + `s` | Snapshot | Shared, by every process with a read transaction on the commit whose transaction id is `s` |

On Windows, byte-range locks are mandatory: bytes locked through one handle cannot be read or written through another. The file never reaches 2^62 bytes, so no read or write ever touches a lock byte. Transaction ids stay below 2^62 − 64, so that every snapshot's byte fits in a signed 64-bit offset; at a million commits a second, that lasts more than 100,000 years.

## One handle per file in each process

On Unix-like systems, a record lock belongs to the process, not to the descriptor it was taken through, and closing any descriptor of the file releases all of the process's locks on it. So each process keeps one shared instance for each database file, which holds the only operating-system handle to the file. Every database object a program opens for that file in that process uses it, and the last one to close closes the file.

- **Files are told apart by what they are**, not by their path: device and inode on Unix-like systems, volume serial number and file index on Windows. Two paths to one file share one instance.
- **Nothing else in the process may open the database file**, not even to copy it. Closing that descriptor would silently drop the database's locks. A backup copies a file safely ([Tools](../guide/tools.md)).
- **A forked child process does not inherit record locks.** Every database object it inherited from its parent fails with `CLOSED`, and the child opens the file itself.
- **The handle that opens a file first** in a process decides the options every handle shares, such as the busy timeout and the size of the page cache.

On Windows, a lock belongs to its handle. With one handle in each process, the behaviour is the same on every platform.

## Opening a file

1. Use the process's instance for the file if it has one, and stop here. Otherwise open the file.
1. Take the recovery lock, trying again with growing pauses; waiting past the busy timeout fails with `BUSY`.
1. Try to take the open lock exclusively, without waiting. Getting it means no other process has the file open: run [recovery](./commits-and-recovery.md#recovery), then turn the open lock into a shared one.
1. Otherwise, other processes have the file open, and the first of them recovered it. Take the open lock shared.
1. Release the recovery lock.

A process holds the open lock only once it has recovered the file or found others holding it, never while it waits to open. If a process dies while it recovers the file after a power cut, its open lock goes with it, and the next process to take the recovery lock finds the file unopened and recovers it itself.

## Reading

A reader takes no lock on the header. It reads, registers, and reads again:

1. Read the first 2048 bytes of the file, and take the record in the slot the selector names. If its check fails, the writer may have been filling that slot, so read again.
1. Take a shared lock on the snapshot's byte, and register the snapshot in the process.
1. Read the selector and the published record again. If they are unchanged, the snapshot is safe to use; otherwise, unregister it and start over.

A writer reuses a page only after a newer commit has stopped using it, and it looks for snapshot locks when it starts. The second read shows that the lock was in place before any such writer started, so that writer finds the lock and leaves the snapshot's pages alone.

When the last read transaction on a snapshot ends, the process keeps the snapshot's lock for 20 milliseconds, so that the next read transaction on the same commit can join it with one read of the header and no lock call: about 1 microsecond to begin instead of 2, on one Apple silicon machine. The `darudb-keeper` thread releases it after that. It goes at once when the process registers a newer snapshot, since no new read transaction can use an older one, and when the process begins a write transaction, so that it holds back none of that writer's pages.

## Writing

1. Take the process's writer gate, so that one thread at a time competes for the file.
1. Take the writer lock exclusively. Each attempt does not block: it is repeated with pauses of at most a millisecond until it succeeds, or until the busy timeout passes, 5 seconds by default, which fails with `BUSY`. Polling is what makes a timeout possible on every platform.
1. Hold both until the commit or the abort has returned.

The writer writes the commit record and the selector with no further lock, since readers verify everything they read. No deadlock is possible: the writer waits for nothing while it holds the writer lock but its own barriers, and readers wait for neither lock.

### Writers take turns

Record locks do not queue waiting processes, and a writer that commits again as soon as it finishes takes the writer lock back before a pausing one wakes. So a writer that has waited 20 milliseconds claims the turn lock, and every other writer, the one that just finished included, lets it go first.

The wait trades one thing for another. A writer that commits now and then beside a busy one waits less with a shorter wait, but processes that commit in tight loops lose a little throughput on every pass of the writer lock. Measured on one Mac against a wait of 50 milliseconds, a writer committing every 10 milliseconds beside a busy one waited 22 milliseconds at the 99th percentile instead of 51, while two processes making deferred commits in tight loops made 7% fewer of them.

## Reclaiming pages

When a write transaction starts, it has to know which retained pages no snapshot can still reach, in any process. It asks one question, whether any snapshot below a given commit is registered, and bisects over the retained groups with it:

- **In this process**, the registry of snapshots answers.
- **In other processes**, on Unix-like systems, `fcntl(F_GETLK)` over the range of snapshot bytes reports whether any lock lies there. It does not report the lowest one, which is why the search bisects.
- **On Windows**, which has no such query, the writer tries to take an exclusive lock over the range without waiting, and releases it at once if it is granted. A reader may wait an instant for such a probe; the writer never waits while it holds one, so the two cannot deadlock.

## When a process dies

The operating system releases its locks, and that is all the cleanup there is:

- **Its snapshots** stop holding pages back, and the next writer reclaims them.
- **The writer lock** passes to the next writer, which starts from the published commit. Whatever the dead writer had written lies at free positions or in a slot that no one reads.
- **The open lock**: if it was the last process with the file open, the next one to open it runs recovery.
- **The recovery lock**, if it was opening the file: the next process waiting for it takes it, and finds the open lock free if the dead one was recovering.
- **The turn lock**: the other writers stop deferring to it.
- **An unsynced window** it left open is ended by another process that notices it, a time limit later ([Commits and recovery](./commits-and-recovery.md#deferred-commits)).

## Platforms

- **Linux and Android**: `fcntl` record locks, through the 64-bit calls on 32-bit targets. An app's private storage supports them. Shared storage served through FUSE may not, and when the lock call reports that locks are unsupported, opening fails with `UNSUPPORTED_FILE_SYSTEM`.
- **macOS and iOS**: `fcntl` record locks, and `F_FULLFSYNC` as the barrier. On iOS, the system terminates a suspended app that holds a file lock inside an App Group container. The open lock is held for as long as the database is open, so an app whose database lives in an App Group container has to close it before the app is suspended. A database in the app's own container is not affected.
- **Windows**: `LockFileEx` and `UnlockFileEx`, with mandatory locks, which is why the lock bytes lie far from the data. There is no way to sync a directory, and none is needed on NTFS, which journals the change itself.
- **Network file systems** are refused with `UNSUPPORTED_FILE_SYSTEM`, since their locks and syncs do not keep the promises the engine relies on. Detection is a best effort: the file system's type on Linux and Android (NFS, SMB and the like), its name on macOS and iOS (`nfs`, `smbfs`, `afpfs`, `webdav`), and UNC paths and remote drives on Windows. A network file system that goes undetected is still not supported.
- **Containers** that share a local volume share the kernel, and so its locks, and work as usual. Hosts that share a volume over a network do not.
