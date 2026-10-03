---
title: Several processes
order: 8
---

# Several processes

Several processes can have one database file open at once, each reading and writing it, with nothing to set up and nothing to clean up after a process dies.

## How they share a file

- Each process sees the others' commits as soon as they are made.
- One process writes at a time, and a reader never waits for a writer.
- A write waits for a writer in another process as it does for one in its own, up to the busy timeout, and then fails with `BUSY`.
- A writer that has waited a moment gets the next turn, so a process that commits in a tight loop cannot keep the others out.

The processes coordinate through the operating system's file locks and nothing else: no shared memory, no lock file, no server. A process that dies at any moment, holding any lock, releases it as it dies, and the others carry on. [Locking](../engine/locking.md) in the Engine section describes the locks.

## Rules that come with the locks

- **Local disks only.** The locks, and the syncs a commit waits for, work only on a local disk. A database on a network file system such as NFS or SMB is refused with `UNSUPPORTED_FILE_SYSTEM`.
- **Do not open the file a second way.** Nothing else in a process that has a database open may open its file, not even to copy it: on Linux and macOS, closing that second handle drops the locks the database holds. To copy a database that is open, use a [backup](./tools.md#back-up-a-file).
- **iOS App Group containers.** An app whose database lives in an App Group container has to close it before the app is suspended, because iOS ends a suspended app that holds a lock there.

## Within one process

Opening a file that is already open in the process gives another handle to the same database: the handles share one page cache and one writer, and the engine keeps one operating-system handle for the file. Each handle keeps the schema it was opened with, and fails with `SCHEMA_MISMATCH` once another handle or process has migrated the file.

::: lang node

In Electron, several instances of the app, or the main process and a utility process, share a file the same way. [Electron](./electron.md) has what is particular to it.

:::

::: lang dart

In a Flutter app, several isolates share a file the same way: each opens its own `Database`, and the handles share the process's page cache and writer. An isolate's asynchronous writes take turns with each other; another isolate's write waits for them in the engine, as another process's would.

:::
