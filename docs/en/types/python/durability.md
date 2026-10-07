---
title: Durability
order: 4
counterpart: /types/node/write-options
---

# Durability

`Durability` says how a write transaction commits: whether the commit waits for the disk.

```python
Durability: TypeAlias = Literal["sync", "deferred"]
```

[`write` and `write_async`](../../api/python/database.md#write) take it as their keyword `durability`, `"sync"` unless it is given. Any other value fails with `INVALID_ARGUMENT` when `write` or `write_async` is called. [Transactions](../../guide/transactions.md) explains the two kinds of commit at length.

```python
with db.write(durability="deferred") as txn:
    txn.collection(User).insert(User(name="Alice"))

# Later, when the users have to survive a power cut:
db.sync()
```

## Values

### sync

The commit returns once it is durable: a power cut after that does not undo it. Each commit costs one sync of the file.

### deferred

The commit returns without waiting for the disk. Readers in every process see it at once, and a crash of this process loses none of it. It becomes durable at the next sync commit, at `sync`, `close` or their `_async` twins, or once the deferred commits since the last sync have waited a second or written 16,384 pages. These are the engine's default limits, which the package does not change. A power cut before then undoes deferred commits from the newest backwards, so the file comes back as it was at an earlier commit, never as a mix.

Deferred commits suit many small writes whose last second may be lost to a power cut, such as a log. A sync commit suits a write the program must not lose once the block ends.
