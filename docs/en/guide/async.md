---
title: Asynchronous API
order: 11
languages: [node]
---

# Asynchronous API

Every method of the Node.js package's `Database` has a twin whose name ends in `Async`, which does the engine's work on the libuv thread pool so that the event loop keeps running.

## Use it

`openAsync`, `readAsync`, `writeAsync`, `syncAsync` and `closeAsync` take what their synchronous forms take and resolve a promise. The event loop keeps running while the engine waits for the disk or for another process's writer, which is why a server should use them.

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

## How operations run

- Every collection method returns a promise. A transaction runs its operations in the order they were called, whether each was awaited or not, and commits only after the last one has settled. A rejected operation changes nothing, as in the synchronous API.
- Operations called together, or while earlier ones are on the pool, go to the engine as one batch in one trip. A trip costs more than most operations, so starting many and awaiting them together is far cheaper than awaiting each in turn: `await Promise.all(keys.map((key) => users.get(key)))`.
- The pool has four threads unless the `UV_THREADPOOL_SIZE` environment variable says otherwise, and Node.js runs its own file system calls there too.

## Writes take turns

- This process's writes on one file run one after another, even through several `Database` objects. A second `writeAsync` waits for the first without holding a thread of the pool, and so do `syncAsync` and `closeAsync`, which wait for the writer when a deferred commit is not yet durable.
- Write transactions still do not nest. Inside a `writeAsync` function, `writeAsync`, `write`, `sync` or `close` on the same file fails with `INVALID_ARGUMENT`, and so do their asynchronous forms.
- While an asynchronous write on the file is under way, a synchronous `write`, `sync` or `close` from anywhere fails the same way, because it would block the event loop that the other write needs to finish.

## Migrations

`openAsync` gives migration functions the same asynchronous collections, and `previous` and `previousKeys` return promises there. A migration function may be asynchronous, and its step ends once every operation it called has settled.
