---
title: Migrating
order: 7
counterpart: /api/dart/migration-context
---

# Migrating

`Migrating` is the write transaction a migration function runs in: the collections of the new schema, and the objects as the schema before the migration read them.

```python
class Migrating(WriteTransaction): ...
```

When `Database.open` finds the file at an older schema version, it migrates the file in one write transaction. It applies every step's renames and the changes the engine makes by itself, then calls the `run` function of each [Migration](./migration.md) between the two versions, in version order, with a `Migrating`. Its `collection` gives the new schema's collections, by class or by name, as a [WriteTransaction](./write-transaction.md) does. If a function raises, nothing of the migration is kept, the file keeps its old schema and data, and `open` raises the same error. [Migrations](../../guide/migrations.md) explains what the engine changes by itself and what needs a function.

The transaction and its collections can be used only while the migration runs; afterwards every operation on them raises `CLOSED`.

```python
import darudb
from darudb import Migration, Migrating


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    age: int = 0


def ages(m: Migrating) -> None:
    users = m.collection(User)

    for key in m.previous_keys("people"):
        before = m.previous("people", key)

        if before is not None:
            users.update(key, age=int(before["age"]))


db = darudb.Database.open(
    "app.darudb",
    schema=darudb.Schema(2, [User]),
    migrations=[
        Migration(
            2,
            rename_collections=[("people", "users")],
            rename_fields=[("people", "fullName", "name")],
            replace_fields=[("people", "age")],
            run=ages,
        )
    ],
)
```

The example is the one of [Migration](./migration.md), whose `age` became an `int`.

## Properties

### previous_version

```python
@property
def previous_version(self) -> int: ...
```

The schema version the file held before the migration. It is the same in every step.

### version

```python
@property
def version(self) -> int: ...
```

The schema version this step migrates to, the `version` of the step's [Migration](./migration.md). A migration from version 1 to 3 runs its step to 2 with `version` 2, then its step to 3 with `version` 3.

## Methods

### previous

```python
def previous(self, collection: str, key: Key) -> dict[str, Any] | None: ...
```

The object of `collection` whose primary key is `key`, as the schema before the migration reads it, or `None`. The collection and the object's fields have the names they had in the file before the migration, and the fields the migration removed or replaced keep their values. The object is a `dict` by those names: an auto-increment collection's key is its `id`, a field the record leaves out holds its default or `None`, an embedded object is a `dict` of its own, a link is the key it holds, and a list is a `list`. A collection the migration deletes can still be read this way until it commits. A collection the old schema did not have fails with `INVALID_ARGUMENT`.

It reads the object as it is now, and writing an object keeps only the new schema's fields, so read an object this way before writing it. The class of the old schema is usually gone from the program, which is why the object is a `dict`.

### previous_keys

```python
def previous_keys(self, collection: str) -> list[Key]: ...
```

The primary keys of every object of `collection`, named as before the migration, in key order: `int`, `str` or `bytes` values, as the collection's key is.

## AsyncMigrating

```python
class AsyncMigrating(AsyncWriteTransaction):
    @property
    def previous_version(self) -> int: ...
    @property
    def version(self) -> int: ...
    async def previous(self, collection: str, key: Key) -> dict[str, Any] | None: ...
    async def previous_keys(self, collection: str) -> list[Key]: ...
```

The write transaction of a migration of `Database.open_async`, whose `run` may be a coroutine function. `previous` and `previous_keys` are coroutines, and `collection` gives an [AsyncWriteCollection](./write-collection.md#asyncwritecollection). Operations run in the order they started, and a step ends once the function has returned, its coroutine included, and every operation it started has finished. The migration holds the file as an asynchronous write does, so a `write_async` on the same file inside the function, through any handle, fails with `INVALID_ARGUMENT` rather than wait for the migration.

```python
from darudb import AsyncMigrating


async def ages(m: AsyncMigrating) -> None:
    users = m.collection(User)

    for key in await m.previous_keys("people"):
        before = await m.previous("people", key)

        if before is not None:
            await users.update(key, age=int(before["age"]))


async def main() -> None:
    db = await darudb.Database.open_async(
        "app.darudb",
        schema=darudb.Schema(2, [User]),
        migrations=[
            Migration(
                2,
                rename_collections=[("people", "users")],
                rename_fields=[("people", "fullName", "name")],
                replace_fields=[("people", "age")],
                run=ages,
            )
        ],
    )
```
