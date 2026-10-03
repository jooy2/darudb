---
title: WriteOptions
order: 2
---

# WriteOptions

`WriteOptions` says how a write transaction commits: whether the commit waits for the disk.

```ts
interface WriteOptions
```

[`write` and `writeAsync`](../../api/node/database.md) take it after the transaction's function. Without it, a commit waits for the disk. [Transactions](../../guide/transactions.md) explains the two kinds of commit at length.

```ts
db.write((txn) => txn.collection('events').insert({ kind: 'click', at: Date.now() }), {
  durability: 'deferred'
});

// Later, when the events have to survive a power cut:
db.sync();
```

## Fields

### durability

```ts
durability?: 'sync' | 'deferred';
```

- **`'sync'`**, the default, returns once the commit is durable: a power cut after that does not undo it. Each commit costs one sync of the file.
- **`'deferred'`** returns without waiting for the disk. Readers in every process see the commit at once, and a crash of this process loses none of it. It becomes durable at the next sync commit, `sync`, `close` or their `Async` twins, or once the deferred commits since the last sync have waited a second or written 16,384 pages. These are the engine's default limits, which the package does not change. A power cut before then undoes deferred commits from the newest backwards, so the file comes back as it was at an earlier commit, never as a mix.

Deferred commits suit many small writes whose last second may be lost to a power cut, such as a log. A sync commit suits a write the program must not lose once the call returns.

Any other value fails with `INVALID_ARGUMENT`, and `writeAsync` rejects with it.
