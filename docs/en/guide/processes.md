---
title: Several processes
order: 10
---

# Several processes

Several processes can have one database file open at once, each reading and writing it, with nothing to set up and nothing to clean up after a process dies.

## How they share a file

- Each process sees the others' commits as soon as they are made.
- One process writes at a time, and a reader never waits for a writer.
- A write waits for a writer in another process as it does for one in its own, up to the busy timeout, and then fails with `BUSY`. The timeout is five seconds unless <LangCode rust="OpenOptions::busy_timeout" node="busyTimeout" dart="busyTimeout" python="busy_timeout" /> says otherwise.
- A writer that has waited a moment gets the next turn, so a process that commits in a tight loop cannot keep the others out.

The processes coordinate through the operating system's file locks and nothing else: no shared memory, no lock file, no server. A process that dies at any moment, holding any lock, releases it as it dies, and the others carry on. [Locking](../engine/locking.md) in the Engine section describes the locks.

## Rules that come with the locks

- **Local disks only.** The locks, and the syncs a commit waits for, work only on a local disk. A database on a network file system such as NFS or SMB is refused with `UNSUPPORTED_FILE_SYSTEM`.
- **Do not open the file a second way.** Nothing else in a process that has a database open may open its file, not even to copy it: on Linux and macOS, closing that second handle drops the locks the database holds. To copy a database that is open, use a [backup](./tools.md#back-up-a-file).
- **iOS App Group containers.** An app whose database lives in an App Group container has to close it before the app is suspended, because iOS ends a suspended app that holds a lock there. A database open in the app's own container is not affected.

::: lang dart

In a Flutter app, close a database in an App Group container when the app goes to the background, and open it again when it is next used. Closing makes deferred commits durable first, and `closeAsync` waits for this isolate's asynchronous writes.

```dart
class SharedDatabase with WidgetsBindingObserver {
  SharedDatabase(this.path) {
    WidgetsBinding.instance.addObserver(this);
  }

  final String path;
  Database? _db;

  Database get db => _db ??= Database.open(path, schema: const Schema(1, [userSchema]));

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused) {
      final db = _db;

      _db = null;
      db?.closeAsync();
    }
  }
}
```

iOS gives an app a few seconds in the background before it suspends it. A write that may take longer has to finish before the app leaves the foreground, or run in background time the app asks iOS for.

:::

## Within one process

Opening a file that is already open in the process gives another handle to the same database: the handles share one page cache and one writer, and the engine keeps one operating-system handle for the file. Each handle keeps the schema it was opened with, and fails with `SCHEMA_MISMATCH` once another handle or process has migrated the file.

::: lang node

In Electron, several instances of the app, or the main process and a utility process, share a file the same way. [Electron](./electron.md) has what is particular to it.

:::

::: lang dart

In a Flutter app, several isolates share a file the same way: each opens its own `Database`, and the handles share the process's page cache and writer. An isolate's asynchronous writes take turns with each other; another isolate's write waits for them in the engine, as another process's would.

:::

::: lang python

Threads can share one `Database`: each handle and transaction keeps its engine object behind a lock, and the native module releases the GIL while the engine works, so other threads run while one waits for the disk or for the writer. A thread's write waits for another thread's, as another process's would, and the package declares itself safe for the free-threaded build of Python.

A child process, whether started with `multiprocessing` or made with `os.fork`, opens the file itself. A handle a forked child inherits from its parent fails there with `CLOSED`, since the child holds none of the locks that handle took.

:::
