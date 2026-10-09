---
title: Commits and recovery
order: 4
---

# Commits and recovery

This page explains how a write transaction changes the file without putting any committed state at risk, when a commit becomes durable, and what opening a file after a crash or a power cut does.

The specification is [design/commits-and-recovery.md](https://github.com/jooy2/darudb/blob/main/design/commits-and-recovery.md) in the repository. [Transactions](../guide/transactions.md) shows how to use them.

## What the engine assumes

Everything below holds on a platform that keeps five promises, and the engine relies on nothing else:

1. **A one-byte write is atomic.** After a power cut, a byte that was being written holds either its old value or its new one.
1. **A write changes only the bytes it names**, whether or not they share a sector with other bytes.
1. **A barrier is a barrier.** When a sync of the file returns successfully, every write issued before it is durable. The engine's barrier is `fsync` on Unix-like systems, `F_FULLFSYNC` on macOS and iOS, where a plain `fsync` only reaches the drive's cache, and `FlushFileBuffers` on Windows.
1. **Byte-range locks work, and die with their owner.**
1. **The file system is local.** Network file systems break promises 3 and 4, so the engine refuses to open a database on one.

Between two barriers, writes may reach the disk in any order, and a write cut short by a power cut may leave any mix of old and new bytes in its range. A page may also be damaged while it sits on the disk. The check on every page catches both.

## Copy on write

A committed page is never changed. To change an entry, the writer writes a new copy of its leaf to a free page, then a new copy of the leaf's parent that points at it, and so on up to the root. The new commit record points at the new roots. Until that record is published, nothing reaches the new pages, so a crash at any moment before it leaves the last commit as it was, and aborting a transaction has nothing to undo.

Within one transaction, only the first change to a committed page copies it; later changes go to the copy, in memory. A large value is written to pages of its own as soon as it is inserted, at free positions that nothing reaches until the commit does.

## Which pages may be reused

Relative to one commit, every page of the file is in exactly one of three states, and the engine's free and retained trees keep the account:

- **Live**: the commit reaches it.
- **Retained**: only an older commit reaches it, which a reader or a recovery may still need. Retained pages are grouped by the commit that stopped using them.
- **Free**: nothing that may still be read reaches it. Only these pages are written.

When a write transaction starts, it frees every retained group that no read transaction, in any process, and no possible recovery can still reach. So a read transaction holds back the pages of its commit for as long as it lives, and the file grows meanwhile if others write, but it never makes a writer wait. Pages that deferred commits wrote and then stopped using are freed sooner, as soon as no reader needs them, so a run of small deferred commits that change the same objects keeps writing the same pages rather than growing the file.

The writer takes free pages lowest first, which keeps the file dense at the front, and a sync commit gives the free pages at the end of the file back to the file system. [Compaction](../guide/tools.md) fills the pages of part-empty trees and moves pages down to give back more.

## Sync commits

A sync commit is durable when it returns: `commit` in Rust, and the default `durability` of a write in Node.js. It costs one barrier:

1. Write every page the transaction changed, each run of consecutive pages with one call.
1. Write the commit record into a slot that holds nothing anyone may need.
1. **Barrier.** This is the commit point: from here on, the commit survives a power cut.
1. Write the selector, naming the new slot. The commit is published, and new readers see it.
1. Cut the free pages at the end off the file.

There is no second barrier after the selector. If a power cut loses that one-byte write, recovery finds the new record newer than the published one, checks it, and publishes it. And since the selector is written only after the barrier, no reader ever sees a sync commit that a power cut could undo.

## Deferred commits

A deferred commit, `commit_deferred` in Rust and `durability: 'deferred'` in Node.js, writes its pages and its record and publishes them at once, with the selector's unsynced bit set and no barrier. Readers see it as soon as it returns. The deferred commits since the last barrier form the unsynced window, which these end with a barrier:

- **The next sync commit**, whose barrier covers the whole window.
- **A call to `sync`, or closing the database**, which calls it. When the last handle to a file in a process is dropped without being closed, the same happens, with no way to report a failure.
- **The page limit.** A deferred commit that would take the window past 16,384 pages, each counted once however often the window wrote it, is made a sync commit instead.
- **The time limit.** Once the window has been open for one second, the `darudb-sync` thread makes it durable, after any write transaction that is running. A deferred commit made after the time is up is made a sync commit as well, in case the thread could not run.
- **Another process.** A process that dies leaves its window open. A process with no window of its own that finds the published commit unsynced, when it begins a read transaction or opens the file, ends that window once it has been open longer than this process's time limit. A process that only holds the file open, and neither reads, writes nor opens it, does not notice.

The limits hold for the window, not for each process. Each deferred commit's record says when its window opened and how many pages the window has written, and a process that makes a deferred commit after another process's goes on counting from there. When processes set different limits, the window ends at the strictest of them. A process cannot tell which pages the others wrote, so a page written again by another process is counted twice, which only brings the barrier sooner. The opening time comes from the system clock, and a time a process cannot place, such as one later than now after the clock was set back, counts as a window already past its time limit.

In Rust, `OpenOptions::max_unsynced_pages` and `OpenOptions::max_unsynced_time` change the limits; the Node.js package uses the defaults. The page limit bounds what the closing barrier has to write and what recovery has to check after a power cut. The time limit bounds how much a power cut can undo.

Deferred commits exist for applications that would rather lose their last few commits in a power cut than wait for the disk on every one of them.

## What a crash can undo

| What happens | Sync commits | Deferred commits |
| --- | --- | --- |
| The process crashes or is killed | Nothing is lost | Nothing is lost: the operating system still holds every write the process made |
| The power is cut | Nothing that returned is lost | The newest may be undone, from the newest back, never leaving a gap |

Either way, the file holds whole commits, never part of one, and the commit it holds is never older than the last sync commit that returned. A power cut never damages the file.

## Choosing a slot

A new record never overwrites a record that a reader or a recovery may still need: the published commit, the durable commit, and the commit that a power cut would make recovery trust without checking. A writer that cannot know which commit that last one is, after opening the file or after another process committed, keeps one more record out of reach instead. Of the slots left, it prefers one that a writer that died before publishing left behind, and otherwise takes the oldest.

When no slot is left, the commit issues a barrier before it writes its record, which leaves only the published and the durable commits to keep. That costs an extra barrier on the second deferred commit after a sync commit made with no window open, and on a writer's first commit after another process has made the first deferred commit of a window.

## When a barrier fails

When a sync fails, the operating system may already have dropped the pages it could not write and marked them clean, so trying again can report a success that did not happen. The engine does not try again:

- The commit fails with `SYNC_FAILED`, and its outcome is unknown: a later recovery may still find and publish it.
- Every later call on that file in this process fails with `SYNC_FAILED`, until every handle to it is closed and the file is opened again, which starts over from what is on the disk. Closing a handle is the last place a failed barrier is reported.
- Other processes are not affected. A sync commit is published only after its barrier succeeds, so they never saw it.

## Recovery

Recovery runs in the first process to open the file, when no other process has it open; the open lock tells ([Locking](./locking.md)). When the last process closed the file normally, recovery costs one read of the header.

1. Read the three commit records, and keep the valid ones whose page count fits in the file. A record that counts more pages than the file holds was written before writes that did not all survive. In an encrypted file, a record whose MAC fails is not valid either.
1. Going from the newest transaction id to the oldest, adopt the first record that is either the published commit with the unsynced bit clear, which is known to be durable, or one that passes checking.
1. If the adopted commit is not the published one, or the unsynced bit was set, or newer records remain, erase the newer records and publish the adopted commit with the unsynced bit clear, with a barrier before and after.
1. Cut the file to the adopted commit's page count.

**Checking a commit** reads only the pages it reaches that were written after its durable commit, which the transaction ids in the pointers point out without reading the rest. For a sync commit that is the pages of that one commit, and after deferred commits the pages of the window. Every one of them has to be intact.

Recovery always finds a commit, because the durable commit always qualifies: its slot is never chosen for a new record, its pages are never reused, and everything it reaches was durable before it was published. If no record qualifies, the file is damaged beyond what recovery repairs, and opening fails with `CORRUPTED`. [Salvage](../guide/tools.md) is for that file.

## Creating a database

A new database is first written whole to a temporary file in the same directory, named after the database with a random suffix, and synced. It is then moved to its name without replacing anything: linked to the name, which fails if the name is taken, or on a file system without links, renamed with the platform's rename that never replaces a file. On Unix-like systems the directory is synced too, so that the new name survives a power cut.

So the path holds either nothing or a whole database, and two processes creating the same database at once cannot collide: the second finds the first one's file and opens it. A file of zero length is never a database and is refused with `NOT_A_DATABASE`, and a temporary file left behind by a crash is never read and is safe to delete.

On a file system with neither links nor such a rename, the engine creates the file in place and writes its first page while holding the open lock alone. A crash in between leaves an empty file, which has to be deleted by hand.
