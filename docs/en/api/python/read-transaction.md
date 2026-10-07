---
title: ReadTransaction
order: 5
---

# ReadTransaction

A `ReadTransaction` reads one commit of the database, for as long as the `with db.read()` block that began it runs.

```python
class ReadTransaction: ...
```

`Database.read` returns a context manager that begins one when its block begins and ends it when the block ends, whether or not the block raised. Everything it reads comes from the commit that was published when it began: commits made meanwhile, by this process or another, are not seen. Beginning one waits for no writer, and no writer waits for it. The transaction and the collections it gives can be used only inside the block; afterwards every operation on them raises `CLOSED`.

Keep read transactions short where others write. While one is open, the pages that later commits stop using cannot be reused, so writers take new pages and the file grows. [Transactions](../../guide/transactions.md) has the longer explanation.

```python
from darudb import Query

with db.read() as txn:
    names = [user.name for user in txn.collection(User).find(Query().sort_by("name"))]
```

## Methods

### collection

```python
@overload
def collection(self, collection: type[T]) -> ReadCollection[T]: ...
@overload
def collection(self, collection: str) -> ReadCollection[Any]: ...
```

The collection of the class `collection`, or of that name, as a [ReadCollection](./read-collection.md) whose objects are instances of the class. Given the class, a type checker knows the type of every object it reads. It raises `INVALID_ARGUMENT` for a class or a name the schema does not have, and when the database was opened without a schema. If another process or handle has migrated the file since this handle opened it, an operation on the collection fails with `SCHEMA_MISMATCH`, and the database has to be opened again with the new schema.

## AsyncReadTransaction

```python
class AsyncReadTransaction:
    @overload
    def collection(self, collection: type[T]) -> AsyncReadCollection[T]: ...
    @overload
    def collection(self, collection: str) -> AsyncReadCollection[Any]: ...
```

The read transaction of `async with db.read_async()`, which sees one commit until its block ends. `collection` works as above, without an `await`, and gives an [AsyncReadCollection](./read-collection.md#asyncreadcollection), whose operations are coroutines that run on the package's thread pool, one at a time, in the order they started. The transaction ends when the block ends, once every operation it started has finished; an operation started after that raises `CLOSED`.

An operation runs when it is awaited, or when a task runs it, as any coroutine does: one that is never awaited never runs.

```python
import asyncio

from darudb import F


async def main() -> None:
    async with db.read_async() as txn:
        users = txn.collection(User)
        alice, adults = await asyncio.gather(users.get(1), users.count(F.age >= 18))
```
