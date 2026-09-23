# Commits and recovery

Status: draft for review.

How a transaction changes the file without ever putting a committed state at risk, when a commit becomes durable, and what opening a file after a crash does. [File format](file-format.md) gives the layout of everything named here, and [Locking](locking.md) says how several processes take turns.

## Transactions

- **A read transaction** sees one commit, its snapshot, for as long as it lives, whatever is committed after it began.
- **A write transaction** starts from the published commit and ends with a new commit or with nothing. There is one at a time per file, across all processes.
- **Readers never wait for the writer, and the writer never waits for readers.** The only thing a reader costs the writer is that the pages the reader can still see are not reused.

## Page states

Relative to one commit, every page from 1 to its page count − 1 is in exactly one of three states:

- **Live**: reachable from the commit's record. That covers the catalog, the free tree, the retained tree, every tree the catalog names, and every overflow run those trees reference.
- **Retained**: not live, but reachable from an older commit that a reader or a recovery may still need. The retained tree lists these pages.
- **Free**: neither. The free tree lists these pages.

Two invariants carry everything else in this document:

1. **Every page is accounted for.** In every commit, the live, retained and free pages together are exactly the pages from 1 to the page count − 1, with no page in two states.
1. **The writer only writes where no one can look.** A write transaction writes only to pages that are free in the commit it started from, or past that commit's page count. It never writes to a page that a registered snapshot, the published commit or the durable commit can reach.

## Copy on write

A committed page is never changed. To change an entry, the writer writes a new copy of the leaf somewhere free. The leaf's parent has to point at the copy, so it writes a new copy of the parent too, and so on up to the root; if the tree's root moved, the catalog's entry for the tree changes, which copies a path of the catalog in the same way. The new commit record points at the new roots. Every page that was copied is no longer live in the new commit, and it joins the new commit's group in the retained tree.

Within one transaction, things are simpler:

- A page the transaction wrote itself is not committed yet, so it can be changed in place, in memory or on disk. Only the first change to a committed page copies it.
- A page the transaction allocated and then stopped using goes straight back to the pool of free pages. No one ever saw it.
- To bound memory, a large transaction may write its pages to disk before it commits. They sit at free positions, so nothing can reach them until the commit record does.

## Reclaiming pages

The retained tree groups pages by the commit that stopped using them. Group `F` holds the pages that were live in the commit `F` was made from and are not live in `F`. A reader whose snapshot is `s` can reach them only if `s < F`, and a recovery that falls back to the durable commit `D` can reach them only if `D < F`.

When a write transaction starts, the writer computes

> `R` = the smallest of `D` and every registered snapshot

and reclaims every group `F ≤ R`: in the commit it is making, those pages move from the retained tree to the free tree. [Locking](locking.md#finding-the-oldest-snapshot) says how the writer finds every registered snapshot, in every process.

The durable commit is in the minimum because recovery may still need it. Until a newer commit is durable, a power cut can return the file to `D`, so nothing `D` can reach may be overwritten.

## Allocating pages

The writer takes pages from the free tree, lowest page numbers first, which keeps the file dense at the front and lets its tail be cut off later. An overflow run needs consecutive pages, so it takes the first free run that is long enough. When nothing free fits, the file grows: new pages start at the page count of the commit the transaction started from, and the new record's page count covers them.

This order is a policy, not part of the format. It may change without a new format version.

## The allocator trees at commit time

Changing the free and retained trees copies their pages too, which frees pages and needs new ones, which changes the trees again. The writer settles this in rounds before it writes the commit record:

1. Finish every change to the user trees and the catalog.
1. Repeat until a round neither allocates nor releases a page:
   1. Delete the reclaimed groups from the retained tree.
   1. Bring the free tree up to date: add the reclaimed runs and the pages released within this transaction, and remove the pages allocated by it.
   1. Write this commit's group into the retained tree, listing every page it stopped using, including the allocator pages copied in the steps above.
1. Compute every page's check from the leaves up, since each parent records its children's checks.

The rounds end quickly because a round can copy an allocator page only the first time it touches it. After that, the page belongs to this transaction and is changed in place.

## Durability

A commit is made in one of two ways:

- **Sync**, the default: durable when the call returns. It costs one barrier.
- **Deferred**: published at once, without a barrier. It becomes durable at the next barrier: the next sync commit, a call to `sync`, the close of the database handle that made it, or the engine's own limit on the unsynced window.

A power cut can undo deferred commits, but only from the newest backwards, never leaving a gap, and it never damages the file. A process that crashes loses nothing, because the operating system still holds everything the process wrote.

Deferred commits exist for the applications that would rather lose their last few commits in a power cut than wait for a barrier on every one of them. Comparing performance fairly with other databases also needs both modes, at the same durability settings on each side.

**Readers see a sync commit only once it is durable**, because it is published after its barrier. A deferred commit is seen before it is durable, which is the trade that mode makes.

**The unsynced window is kept short.** The engine issues a barrier of its own once the window holds more than a set number of pages or has been open longer than a set time. That bounds how much a power cut can undo and how long recovery's check takes. The defaults come from the benchmarks.

The storage kernel may ship sync commits first. The format supports deferred commits from the start, so adding them later needs no new format version.

## The durable commit

The writer and recovery both need to know which commit is durable:

- If the selector's unsynced bit is clear, the durable commit is the published one.
- If it is set, the durable commit is the record whose transaction id equals the published record's durable transaction id. That record is always present, because a slot holding the durable commit is never chosen for a new record.

## Starting a write transaction

1. Take the writer lock ([Locking](locking.md#writing)).
1. Read the selector and the three records.
1. Find the durable commit `D`, as above.
1. Choose the slot for the new record, and the new transaction id `T`, as below.
1. Compute `R` and reclaim the groups it allows.

The transaction's base is the published commit.

## Choosing the slot

The new record goes into a slot that holds neither the published commit nor the durable commit. So it never overwrites a record that a reader or a recovery may need.

- With the unsynced bit set, exactly one slot qualifies.
- With it clear, two do. The writer prefers one holding a record newer than the published commit, which a writer that died before publishing left behind. Otherwise it takes the older of the two, treating an empty slot as the oldest.

The new transaction id `T` is one more than the largest transaction id in any valid record, not in the published one only. That way a record left behind by a dead writer never shares an id with a new commit.

## A sync commit

1. Finish the trees, so that every page's final content and check are known.
1. Write every page of the transaction that is not on disk already.
1. Write the commit record into the chosen slot, with `T`, the durable transaction id of `D`, the page count, the three roots and the key block.
1. **Barrier.** This is the commit point: from here on, the commit survives a power cut whatever happens to the selector.
1. Write the selector: the chosen slot, with the unsynced bit clear. The commit is now published.
1. If the file is longer than the commit's page count, truncate it. The pages past the end are free, so no snapshot and no recovery can need them. A deferred commit never truncates; the next sync commit does.
1. Return.

There is no second barrier after the selector. If a power cut loses the selector write, recovery finds the new record newer than the published one, checks it, and publishes it.

## A deferred commit

1. Finish the trees.
1. Write every page of the transaction that is not on disk already.
1. Write the commit record into the chosen slot, with `T` and the durable transaction id of `D`.
1. Write the selector: the chosen slot, with the unsynced bit set. The commit is published but not yet durable.
1. Return.

The record's durable transaction id keeps pointing at `D`, which keeps `D`'s slot out of reach and `D`'s pages from being reclaimed until the window ends.

## Ending the unsynced window

A barrier, followed by writing the selector with the unsynced bit clear, makes the published commit the durable one. Four things do this: a sync commit, whose own barrier covers the whole window; a call to `sync`; closing a database handle that made deferred commits; and the engine's window limit. Only the writer writes the selector, so `sync` takes the writer lock first.

## When a barrier fails

When a sync fails, the operating system may already have dropped the pages it could not write and marked them clean, so trying again can report a success that did not happen. The engine therefore does not try again:

- The commit fails with `SYNC_FAILED`. Its outcome is unknown: the commit may still be found and published by a later recovery.
- The process's handle to the file becomes unusable, and every later call on it fails with `SYNC_FAILED`. Closing every `Database` object for the file and opening it again starts over with what is on disk.
- Other processes are unaffected. A sync commit is published only after its barrier succeeds, so they never saw it.

## Aborting

The writer throws away the transaction's pages. Any that were already written to disk sit at free positions that nothing reaches, so nothing on disk has to be undone, and the selector is not touched.

## Recovery

Recovery runs when a process opens the file and finds that no other process has it open ([Locking](locking.md#opening-and-closing)). When the last process closed normally it costs one read of the header.

### Choosing the commit

1. Read the static fields, the selector and the three records. Keep the valid records whose page count fits in the file.
1. Go through them from the newest transaction id to the oldest, and adopt the first one that is either
   - the published commit, with the unsynced bit clear, which is known to be durable; or
   - a commit that passes [checking](#checking-a-commit).
1. If none qualifies, the file is damaged beyond what recovery repairs: `CORRUPTED`.
1. If the adopted commit is not the published one, or the unsynced bit was set: issue a barrier, erase every record newer than the adopted one, write the selector with the adopted slot and the unsynced bit clear, and issue another barrier.
1. If the file is longer than the adopted commit's page count, truncate it.

Recovery always finds a commit, because the durable commit always qualifies. Its record is never overwritten, since its slot is never chosen. Its pages are never reused, since `R` is never above it. And it is either published with the unsynced bit clear, or it passes checking: the pages its own window wrote are live in it and therefore intact, and every older page it reaches was durable before it was written.

What the caller can rely on: the commit recovery adopts is never older than the last sync commit that returned. After a process crash without a power cut, it is at least the newest published commit, so no commit that returned is lost, deferred ones included.

### Checking a commit

Checking commit `c` confirms that the pages `c` reaches, among those written after its durable transaction id `L`, all reached the disk intact. Pages written at or before `L` were durable before `c` was written, and `c` still reaches them, so nothing can have overwritten them. They are not read.

Starting from each of `c`'s three root pointers whose transaction id is above `L`:

1. The page lies below `c`'s page count. Read it.
1. Its check matches the pointer's check, its header's transaction id matches the pointer's, and its kind, level and tree id are what the parent expects.
1. For a branch, go on with every child pointer whose transaction id is above `L`.
1. For a leaf, check every overflow run it references whose transaction id is above `L`: each page's own check, and the run check over them.
1. For a leaf of the catalog, go on with every tree root it names whose transaction id is above `L`.

`c` passes if nothing fails. The cost is proportional to the pages written in `c`'s unsynced window that `c` still reaches. For a sync commit, that is the pages of that one commit. The transaction ids in the pointers are what let the check skip every page it does not need, without reading it.

## Creating a database

1. Build page 0: the static fields with a fresh random file id, slot 0 holding the first commit (transaction id 1, durable transaction id 0, page count 1, null roots, next tree id 16), slots 1 and 2 empty, and the selector pointing at slot 0 with the unsynced bit clear.
1. Write it to a new temporary file in the same directory, named after the database with a random suffix, and issue a barrier.
1. Move the temporary file to the database's name **without replacing anything**: `renameat2` with `RENAME_NOREPLACE` on Linux and Android, `renamex_np` with `RENAME_EXCL` on macOS and iOS, `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING` on Windows. Where the file system refuses those, link the temporary file to the name, which also fails if the name exists, and remove the temporary file.
1. If the name is taken, another process created the database first: remove the temporary file and open the existing one as usual.
1. On Unix-like systems, issue a barrier on the directory, so that the new name survives a power cut.

So the file at the path is either absent or a complete database, and two processes creating it at once cannot collide. A file of zero length is never a database and is refused with `NOT_A_DATABASE`.

On the rare file system that supports neither a no-replace move nor links, the engine falls back to creating the file directly and writing page 0 while holding the open lock exclusively. A crash in between leaves an empty file, which is refused as above, and the file has to be deleted by hand.

A temporary file left behind by a crash is never read by the engine and is safe to delete.

## Checking and salvaging

The integrity check, a tool of phase 6, verifies the published commit completely:

- Every page it reaches verifies against its pointer, with consistent kinds, levels and tree ids. Keys are in order within each node and lie between the separators on their path. Overflow runs are whole, and every tree's entry count matches its descriptor.
- Every retained group was made by a commit no newer than the published one, and every run lies below the page count.
- The live, retained and free pages cover every page from 1 to the page count − 1 exactly once. A page in none of them has leaked; a page in two is about to be overwritten while in use.

It reports every problem it finds rather than stopping at the first.

Salvage, also phase 6, rebuilds what it can from a file the check rejects. The format prepares for it: a plain page's check covers its own page number, an encrypted page authenticates itself with the key, and every page header names the page's kind, level, tree and commit. A salvage tool can therefore scan every page, keep those that verify, choose the newest version of each part of each tree, and write the result into a new file. The algorithm is designed with the tool.

## Errors

This document adds one error code to the engine:

| Code          | When                                                                                    |
| ------------- | --------------------------------------------------------------------------------------- |
| `SYNC_FAILED` | A barrier failed. The commit's outcome is unknown, and the file has to be opened again. |

It also uses `CORRUPTED`, for a file recovery cannot repair or a page that fails verification, and `IO`, for any other failure the operating system reports.

## What this means for the API

The storage kernel's API is settled when it is written. The design implies at least:

- A read transaction and a write transaction, each seeing one snapshot.
- `commit`, which is a sync commit, and a way to make a deferred commit.
- `Database::sync`, which ends the unsynced window.
- Options for the time to wait for the writer lock and for the unsynced window's limits.

## What the phase 1 tests must show

These are the phase 1 exit criterion, "no data lost across thousands of repeated `kill -9` and simulated power cuts", made specific.

- **A simulated disk.** The `storage` layer reaches the file through an interface the tests replace with a disk that keeps two images: what the operating system holds and what is durable. A barrier copies the first to the second. A simulated power cut discards the first and keeps any subset of the writes made since the last barrier, each one either whole, absent, or torn into any mix of old and new bytes within its own range. A one-byte write is whole or absent, and nothing outside a write's range ever changes.
- **Power cuts.** Over thousands of random sequences of operations, cut at random points: the file opens; its contents equal the state after some commit that is no older than the last sync commit that returned, and no newer than the last commit attempted; and the integrity check passes.
- **Process kills.** Real child processes are killed with `SIGKILL` on Unix-like systems and `TerminateProcess` on Windows at random points. After the file is opened again, every commit that returned is present, deferred ones included.
- **Damage.** Random bits flipped in random pages make reads fail with `CORRUPTED`, never with a panic, and the integrity check names the damaged page.
- **Encryption.** From phase 2, every suite above runs with encryption on and off.
