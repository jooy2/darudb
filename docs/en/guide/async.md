---
title: Asynchronous API
order: 13
languages: [node, dart, python]
---

# Asynchronous API

Every method of `Database` that uses the file has a twin whose name ends in <LangCode node="Async" dart="Async" python="_async" />, which does the engine's work off the thread that runs your code, so that it keeps running while the engine waits.

## Which one to use

Both forms do the same work. They differ in where it runs, and that decides what each costs.

- **The synchronous form costs less per call.** The engine works on the calling thread and the call returns its result, with no trip to another thread and back. A read by key, or a deferred commit of a few objects, finishes sooner than that trip would.
- **The synchronous form holds the calling thread until it returns.** Nothing else runs on that thread in the meantime.

Most calls return in microseconds, but some can hold the thread for much longer:

- A sync commit waits for the disk.
- A write, and opening a file, wait for another process's writer, up to the busy timeout: 5 seconds by default.
- Opening a file may run recovery or migrations, and opening an encrypted file with a password spends tens of milliseconds turning it into a key.
- A query that returns many objects takes as long as reading them, and the tools, which check, back up, compact or salvage, read the whole file.

::: lang node

- **A server, or Electron's main process**: use the asynchronous form for writes, for opening and for the tools. While a synchronous call waits, a server answers no other request, and an Electron app's windows get no answer to what they ask the main process for. Reads by key and small queries can stay synchronous.
- **A script or a command-line tool** that does one thing at a time: use the synchronous form. Nothing else is waiting for the thread, so it is simpler and faster.

:::

::: lang dart

- **A Flutter app's UI isolate**: use the `Future` form for writes, for opening and for the tools. While a synchronous call runs, the isolate draws no frame, and a frame at 60 Hz has about 16 milliseconds: opening with a password takes longer than that, and a write that waits for another process can take seconds. Reads by key and small queries can stay synchronous.
- **A background isolate of your own** can use the synchronous form for heavy work without holding the UI. It opens its own `Database` on the file, as [Several processes](./processes.md) describes, and its results reach the UI isolate as messages.
- **A Dart server or command-line tool**: a server that handles several requests at once uses the `Future` form, and a tool that does one thing at a time uses the synchronous form.

:::

::: lang python

- **A program on an `asyncio` event loop, such as a server**: use the asynchronous form for writes, for opening and for the tools. While a synchronous call waits, the loop runs no other task, so a server answers no other request. Reads by key and small queries can stay synchronous.
- **A script, a command-line tool, or a program that uses threads**: use the synchronous form. The native module releases the GIL while the engine works, so the program's other threads keep running while one waits for the disk or for another writer.

:::

Both forms can be used on one file in one process. A synchronous write is refused while an asynchronous one holds the file, as [Writes take turns](#writes-take-turns) explains.

## Use it

::: lang node

`openAsync`, `readAsync`, `writeAsync`, `syncAsync` and `closeAsync` take what their synchronous forms take and resolve a promise. The engine's work runs on the libuv thread pool, so the event loop keeps running while the engine waits for the disk or for another process's writer, which is why a server should use them.

```ts
const db = await Database.openAsync('app.darudb', { schema: app });

const key = await db.writeAsync(async (txn) => {
  const users = txn.collection('users');
  const bob = await users.findOne((q) => q.where('name', '==', 'Bob'));

  if (bob !== null) {
    await users.put({ ...bob, age: bob.age + 1 });
  }

  return users.insert({ name: 'Carol' });
});

const adults = await db.readAsync((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18))
);
```

- The function may be asynchronous. `writeAsync` commits when it resolves and aborts when it rejects, and takes the same `durability` option as `write`. `readAsync` sees one commit until the function settles.
- `readAsync` begins its read on the calling thread, as `read` does, because beginning a read waits for no writer and costs less than a trip to the pool.
- The tools have twins too: `checkAsync`, `backupAsync`, `compactAsync`, `setKeyAsync`, `setPasswordAsync`, and `Database.salvageAsync`.

:::

::: lang dart

`openAsync`, `readAsync`, `writeAsync`, `syncAsync` and `closeAsync` take what their synchronous forms take and return a `Future`. The engine's work runs on threads of the package's native library, and the result comes back on the isolate's event loop, so a Flutter app's UI isolate never waits for the disk or for another writer.

```dart
final db = await Database.openAsync('app.darudb', schema: const Schema(1, [userSchema]));

final key = await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);
  final bob = await users.findOne((q) => q.where(q.name.equals('Bob')));

  if (bob != null) {
    await users.put(bob.copyWith(age: bob.age + 1));
  }

  return users.insert(const User(name: 'Carol'));
});

final adults = await db.readAsync(
  (txn) => txn.collection(userSchema).find((q) => q.where(q.age.atLeast(18))),
);
```

- The function may be asynchronous. `writeAsync` commits when it completes and aborts when it fails, and takes the same `durability` as `write`. `readAsync` sees one commit until the function completes.
- `readAsync` begins its read on the calling isolate, as `read` does, because beginning a read waits for no writer.
- The tools have twins too: `checkAsync`, `backupAsync`, `compactAsync`, `setKeyAsync`, `setPasswordAsync`, and `Database.salvageAsync`.

:::

::: lang python

`Database.open_async`, `sync_async` and `close_async` take what their synchronous forms take and are awaited, and `db.read_async()` and `db.write_async()` are used with `async with`, where every collection method is awaited. The engine's work runs on a thread pool the package keeps, and the native module releases the GIL there, so the event loop keeps running while the engine waits for the disk or for another process's writer.

```python
import darudb
from darudb import F

db = await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User]))

async with db.write_async() as txn:
    users = txn.collection(User)
    bob = await users.find_one(F.name == "Bob")

    if bob is not None:
        await users.update(bob.id, age=bob.age + 1)

    key = await users.insert(User(name="Carol"))

async with db.read_async() as txn:
    adults = await txn.collection(User).find(F.age >= 18)

await db.close_async()
```

- A `write_async` block commits when it ends, once every operation started in it has finished, and aborts when it raises. It takes the same `durability` as `write`. A `read_async` block sees one commit until it ends.
- `read_async` begins its read on the event loop's thread, as `read` does, because beginning a read waits for no writer. `write_async` begins its write on the pool, since that waits for the writer.
- A database is an asynchronous context manager too: `async with await darudb.Database.open_async(...) as db:` closes it with `close_async` when the block ends.
- The tools have twins too: `check_async`, `backup_async`, `compact_async`, `set_key_async`, `set_password_async`, and `Database.salvage_async`.

:::

## How operations run

::: lang node

- Every collection method returns a promise. A transaction runs its operations in the order they were called, whether each was awaited or not, and commits only after the last one has settled. A rejected operation changes nothing, as in the synchronous API.
- Operations called together, or while earlier ones are on the pool, go to the engine as one batch in one trip. A trip costs more than most operations, so starting many and awaiting them together is far cheaper than awaiting each in turn: `await Promise.all(keys.map((key) => users.get(key)))`.
- The pool has four threads unless the `UV_THREADPOOL_SIZE` environment variable says otherwise, and Node.js runs its own file system calls there too.

:::

::: lang dart

- Every collection method returns a `Future`. A transaction runs its operations in the order they were called, whether each was awaited or not, and commits only after the last one has completed. A failed operation changes nothing, as in the synchronous API.
- Each operation is a trip to a thread of the library and back, which costs more than most operations do. A batch, such as `insertMany`, makes one trip for all of its objects.
- The library's threads grow in number when every one is busy, so a write that waits for another one never holds the last thread the other needs, and end after ten seconds with nothing to do.

:::

::: lang python

- Every collection method is a coroutine. A transaction runs its operations one at a time, in the order they start, so `asyncio.gather` of several inserts runs them in the order it is given them. A refused operation changes nothing, as in the synchronous API.
- Each operation is a trip to a thread of the pool and back, which costs more than most operations do. A batch, such as `insert_many`, makes one trip for all of its objects. Operations started together are not batched: each is its own trip, one after another.
- The pool is the package's own, apart from the event loop's default executor. It makes a thread only when every thread it has is busy, up to 32 threads or the number of processors plus four, whichever is fewer.

:::

## Writes take turns

::: lang node

- This process's writes on one file run one after another, even through several `Database` objects. A second `writeAsync` waits for the first without holding a thread of the pool, and so do `syncAsync` and `closeAsync`, which wait for the writer when a deferred commit is not yet durable.
- Write transactions still do not nest. Inside a `writeAsync` function, `writeAsync`, `write`, `sync` or `close` on the same file fails with `INVALID_ARGUMENT`, and so do their asynchronous forms.
- While an asynchronous write on the file is under way, a synchronous `write`, `sync` or `close` from anywhere fails the same way, because it would block the event loop that the other write needs to finish.

:::

::: lang dart

- An isolate's asynchronous writes on one file run one after another, even through several `Database` objects. A second `writeAsync` waits for the first, and so do `syncAsync`, `closeAsync`, `compactAsync` and the key changes, which wait for the writer too.
- Write transactions still do not nest. Inside a `writeAsync` function, a write, sync, close or compaction on the same file fails with `INVALID_ARGUMENT`, synchronous or not.
- While an asynchronous write on the file is under way, a synchronous `write`, `sync`, `close` or `compact` fails the same way, because it would hold the isolate the other write needs to finish.
- Another isolate's writes are another queue: they wait for this isolate's in the engine, as another process's would.

:::

::: lang python

- An event loop's asynchronous writes on one file run one after another, even through several `Database` objects. A second `write_async` waits for the first on the event loop, without holding a thread of the pool, and so do `sync_async`, `close_async`, `compact_async` and the key changes, which wait for the writer too.
- Write transactions still do not nest. `write_async` inside a synchronous `write` block on the same file fails with `INVALID_ARGUMENT`. Inside a `write_async` block, `write_async`, `sync_async`, `close_async`, `compact_async` or a key change on the same file fails the same way, since each would wait its turn after the block, and the block would wait for itself. A task made inside the block counts as inside it.
- While an asynchronous write on the file is under way, a synchronous `write`, `sync`, `close`, `compact`, `set_key` or `set_password` on the event loop's thread fails with `INVALID_ARGUMENT`, because it would hold the thread the other write needs to finish. From another thread, it waits for the writer as another process's write would.
- Another event loop's writes, in another thread, are another queue: they wait for this loop's in the engine, as another process's would.

:::

## Migrations

::: lang node

`openAsync` gives migration functions the same asynchronous collections, and `previous` and `previousKeys` return promises there. A migration function may be asynchronous, and its step ends once every operation it called has settled.

:::

::: lang dart

With `openAsync`, a migration function may be asynchronous, and its step ends once the function's `Future` completes. The calls it makes on its `MigrationContext` are synchronous, as with `open`: the migration holds the writer, so nothing they wait for is another writer.

:::

::: lang python

With `open_async`, a migration's `run` may be a coroutine function. It gets an `AsyncMigrating`, whose collections are asynchronous and whose `previous` and `previous_keys` are awaited, and its step ends once the function and every operation it started have finished. `open` refuses a coroutine function with `INVALID_ARGUMENT`.

:::
