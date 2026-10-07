---
title: WriteCollection
order: 9
counterpart: /api/rust/collection-writer
---

# WriteCollection

A `WriteCollection` writes the objects of one collection in a write transaction, and reads them as a `ReadCollection` does.

```python
class WriteCollection(ReadCollection[T]): ...
```

`collection` of a write transaction or of a [Migrating](./migrating.md) returns one. Reads see the transaction's own changes. The methods can be called only inside the transaction's block; afterwards they raise `CLOSED`.

Every write converts the object to the engine's values and checks it against the schema first:

- **`INVALID_ARGUMENT`** for an object that does not fit: an object that is not an instance of the collection's class, a value of another type than its field's, `None` in a required field, an `int` beyond 64 bits, a `bool` in an `int` field, `None` in a list, or a link holding a key of another type than the linked collection's. [Field types](../../types/python/field-types.md) says what each field takes.
- **`DUPLICATE_KEY`** when an insert finds its key taken, or a unique index finds one of the object's values taken.
- **A refused write changes nothing**, and a block that catches the error can go on and commit.

A write returns a [Key](../../types/python/key.md): an `int`, a `str` or `bytes`. [Objects](../../guide/objects.md) has the rules the engine keeps.

```python
with db.write() as txn:
    users = txn.collection(User)
    alice, bob = users.insert_many(
        [User(name="Alice", email="alice@example.com", age=31), User(name="Bob", tags=["new"])]
    )

    users.update(alice, age=32, email=None)
    users.put(User(id=2, name="Robert", age=18))
    users.delete(bob)
```

## Methods

### insert

```python
def insert(self, obj: T) -> Key: ...
```

Inserts `obj` and returns its primary key. In a collection keyed by an auto-increment, an object whose `id` is `None` gets the next number; one with an `id` is inserted under it.

### insert_many

```python
def insert_many(self, objects: Iterable[T]) -> list[Key]: ...
```

Inserts `objects`, any iterable of them, in one call into the engine and returns their keys, in order. Something that is not iterable fails with `INVALID_ARGUMENT`. Every object is converted before any is inserted, so one that does not convert refuses the batch before anything is written. An object the engine refuses stops the batch with its error, and the objects before it stay inserted in the transaction, which commits them unless the block raises. The native module releases the GIL once for the batch, rather than once for each object.

### put

```python
def put(self, obj: T) -> Key: ...
```

Inserts `obj`, or replaces the object with its key, and returns the key. It fails as `insert` does, except that a taken key is not a failure. The object replaces the stored one whole. An object of an auto-increment collection whose `id` is `None` is inserted with the next number.

### put_many

```python
def put_many(self, objects: Iterable[T]) -> list[Key]: ...
```

`put` for each object, in one call into the engine, with the batch rules of `insert_many`.

### update

```python
def update(self, key: Key, /, **changes: object) -> bool: ...
```

Sets the fields that `changes` names, by their attributes, in the object whose primary key is `key`, and returns whether there was one; when there is none, nothing is written. The rest of the object stays as it is. `key` is positional only, so a field called `key` can be changed too.

- `None` makes an optional field `None`, and gives a field with a default its default. A required field without a default cannot be set to `None`.
- An embedded object or a list is replaced whole: `update(1, address=Address(city="Seoul"))`.
- A field declared with `field(name=...)` is named by its attribute, and the package gives the engine the stored name.

It is refused as `put` is: with `INVALID_ARGUMENT` for an attribute the class does not have or a value of another type, and with `DUPLICATE_KEY` for a unique value another object holds. Changing the primary key fails with `INVALID_ARGUMENT`; the key the object already has is accepted. Only the changed fields are converted and cross into the engine.

```python
with db.write() as txn:
    txn.collection(User).update(1, age=37, email=None, tags=["admin"])
```

### delete

```python
def delete(self, key: Key) -> bool: ...
```

Deletes the object whose primary key is `key`, with its index entries, and returns whether there was one. A key that is not an `int`, a `str` or bytes fails with `INVALID_ARGUMENT`.

## AsyncWriteCollection

```python
class AsyncWriteCollection(AsyncReadCollection[T]):
    async def insert(self, obj: T) -> Key: ...
    async def insert_many(self, objects: Iterable[T]) -> list[Key]: ...
    async def put(self, obj: T) -> Key: ...
    async def put_many(self, objects: Iterable[T]) -> list[Key]: ...
    async def update(self, key: Key, /, **changes: object) -> bool: ...
    async def delete(self, key: Key) -> bool: ...
```

The collection of an asynchronous write transaction, with the reads of [AsyncReadCollection](./read-collection.md#asyncreadcollection). Its members do what the members above do as coroutines; every failure raises the same error where the operation is awaited, and a refused operation changes nothing. Operations run in the order they started, and one that fails does not stop the ones after it. Each operation is a trip to the package's thread pool and back, so a batch of objects belongs in one `insert_many` or `put_many`.

```python
import asyncio


async def main() -> None:
    async with db.write_async() as txn:
        users = txn.collection(User)

        await asyncio.gather(users.insert(User(name="Dave")), users.update(1, age=33), users.delete(3))
```
