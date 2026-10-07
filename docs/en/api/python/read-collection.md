---
title: ReadCollection
order: 8
counterpart: /api/rust/collection-reader
---

# ReadCollection

A `ReadCollection` reads the objects of one collection in a transaction: by primary key, and by query.

```python
class ReadCollection(Generic[T]): ...
```

`collection` of a read transaction returns one, and a write transaction's [WriteCollection](./write-collection.md) has every member below too, reading the transaction's own changes. `T` is the collection's class. The methods can be called only inside the transaction's block; afterwards they raise `CLOSED`.

Objects come back as instances of the class that outlive the transaction, built without its `__init__`, as [collection](./collection.md#objects-read-are-built-without-init) explains. A field a record leaves out, written before the field existed, holds its default or `None`, and a collection keyed by an auto-increment gives each object its `id`. [Field types](../../types/python/field-types.md) says what each field reads as.

## Properties

### name

```python
@property
def name(self) -> str: ...
```

The collection's name, as the file stores it.

## Methods

### get

```python
def get(self, key: Key) -> T | None: ...
```

The object whose primary key is `key`, or `None`. A [Key](../../types/python/key.md) is an `int`, a `str` or bytes, of the type of the collection's key. A key of another type, a `bool`, a `float` or `None` fails with `INVALID_ARGUMENT`.

### find

```python
def find(self, query: QueryInput = None, /, *parameters: object) -> list[T]: ...
```

The objects a query finds, in its order. A query takes one of these forms, which [QueryInput](../../types/python/query-input.md) declares:

- **Nothing**: every object, in primary key order.
- **A [Query](./query.md)**, as `where(F.age >= 18).sort_by(F.age)`.
- **A [condition](./conditions.md)**, as `F.age >= 18`, which stands for the query with that condition alone.
- **Text** in the [query language](./query.md#the-query-language), where `$0`, `$1` and on take the values of `parameters` in order. The package keeps the last 256 texts it has parsed, for every database of the process, so a text that runs again is not parsed again.
- **A [Prepared](../../types/python/prepared.md) query** that `Database.prepare` made on this collection.

`parameters` come after the query, as positional arguments: `find("age >= $0", 18)`. A built query with [param](./param.md) in place of values takes them the same way, prepared or not. A query that has no parameters ignores the values it is given.

A query that does not fit fails with `INVALID_QUERY`: a field the collection does not have, a value of another type, text that does not parse, a query prepared on another collection, a parameter without a value, or something that is not a query at all.

```python
from darudb import F, Query, where

adults = where(F.age >= 18)

with db.read() as txn:
    users = txn.collection(User)

    users.find()
    users.find(F.tags.contains("new"))
    users.find(adults.sort_by(F.age, descending=True).limit(10))
    users.find(Query().sort_by(F.name))
    users.find("age >= $0 AND name STARTSWITH $1", 18, "A")
```

A query built with `F` is compiled the first time it runs with a collection's class, and the query keeps what it compiled for each class and schema, so a query kept in a variable and run again is not compiled again.

### find_one

```python
def find_one(self, query: QueryInput = None, /, *parameters: object) -> T | None: ...
```

The first object a query finds, or `None`. It takes the query in the same forms as `find`, and the engine stops reading at the first object. A query's own offset and limit still apply, so a limit of 0 finds nothing.

### count

```python
def count(self, query: QueryInput = None, /, *parameters: object) -> int: ...
```

How many objects a query finds, after its offset and within its limit. Without a query it counts every object, which reads only the number the collection keeps rather than the objects.

## AsyncReadCollection

```python
class AsyncReadCollection(Generic[T]):
    @property
    def name(self) -> str: ...
    async def get(self, key: Key) -> T | None: ...
    async def find(self, query: QueryInput = None, /, *parameters: object) -> list[T]: ...
    async def find_one(self, query: QueryInput = None, /, *parameters: object) -> T | None: ...
    async def count(self, query: QueryInput = None, /, *parameters: object) -> int: ...
```

The collection of an asynchronous transaction. Its members do what the members above do as coroutines, and every failure, `CLOSED` included, raises the same error where the operation is awaited. Operations run on the package's thread pool one at a time, in the order they started, each a trip of its own.

```python
import asyncio


async def main() -> None:
    async with db.read_async() as txn:
        users = txn.collection(User)
        found = await asyncio.gather(*(users.get(key) for key in (1, 2, 3)))
```
