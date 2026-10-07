---
title: WriteTransaction
order: 6
---

# WriteTransaction

A `WriteTransaction` holds changes to the database that commit together when the `with db.write()` block that began it ends.

```python
class WriteTransaction(ReadTransaction): ...
```

`Database.write` returns a context manager that begins one when its block begins, commits it when the block ends and aborts it when the block raises, so nothing a block that raised did is kept, and the error goes on. The transaction reads the commit it began at together with its own changes. A write that is refused, with `DUPLICATE_KEY` or `INVALID_ARGUMENT`, changes nothing, and a block that catches the error can go on and commit the rest. The transaction and its collections can be used only inside the block; afterwards every operation on them raises `CLOSED`.

One write transaction runs on a file at a time, across every thread and process. Beginning one waits for another writer, in another thread or another process, for up to `busy_timeout`, five seconds by default, and then fails with `BUSY`. Within a thread, write transactions do not nest: a `with db.write()` inside another's block on the same file, through this handle or another, fails at once with `INVALID_ARGUMENT`, rather than wait for itself. So do `sync`, `close`, `compact`, `set_key` and `set_password` on the same file inside the block, which would wait for it too.

The commit is durable when the block ends, unless `db.write(durability="deferred")` asked for a deferred commit. That one returns without waiting for the disk: readers see it at once, a crash of the process loses none of it, and it becomes durable at the next sync commit, `sync` or `close`, or once the deferred commits since the last sync have waited a second or written 16,384 pages. [Durability](../../types/python/durability.md) describes both, and [Transactions](../../guide/transactions.md) has the longer explanation.

```python
with db.write() as txn:
    users = txn.collection(User)
    key = users.insert(User(name="Alice", email="alice@example.com"))

    users.update(key, age=31)
```

## Methods

### collection

```python
@overload
def collection(self, collection: type[T]) -> WriteCollection[T]: ...
@overload
def collection(self, collection: str) -> WriteCollection[Any]: ...
```

The collection of the class `collection`, or of that name, as a [WriteCollection](./write-collection.md), which reads the transaction's own changes and makes more. It raises `INVALID_ARGUMENT` for a class or a name the schema does not have, and when the database was opened without a schema.

## AsyncWriteTransaction

```python
class AsyncWriteTransaction(AsyncReadTransaction):
    @overload
    def collection(self, collection: type[T]) -> AsyncWriteCollection[T]: ...
    @overload
    def collection(self, collection: str) -> AsyncWriteCollection[Any]: ...
```

The write transaction of `async with db.write_async()`. It begins on the package's thread pool, once this event loop's earlier asynchronous writes on the file have ended. It commits when the block ends, once every operation it started has finished, and aborts when the block raises. `collection` works as above and gives an [AsyncWriteCollection](./write-collection.md#asyncwritecollection), whose operations are coroutines that run on the pool in the order they started.

An operation that fails raises its error where it is awaited and changes nothing, and if the block catches it, the transaction still commits the rest. While the block runs, a synchronous `write`, `sync`, `close`, `compact`, `set_key` or `set_password` on the same file from the event loop's thread is refused with `INVALID_ARGUMENT`, since it would wait for a write that needs that event loop to end. So is a `write_async`, `sync_async`, `close_async`, `compact_async`, `set_key_async` or `set_password_async` on the same file awaited inside the block, or in a task made inside it, since it would wait for the write it is part of.

```python
import dataclasses

from darudb import F


async def main() -> None:
    async with db.write_async() as txn:
        users = txn.collection(User)
        bob = await users.find_one(F.name == "Bob")

        if bob is not None:
            await users.put(dataclasses.replace(bob, age=bob.age + 1))

        await users.insert(User(name="Carol"))
```
