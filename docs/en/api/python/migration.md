---
title: Migration
order: 4
counterpart: /types/node/migration
---

# Migration

`Migration` says what schema version `version` changes from the version before it, beyond what the engine works out by itself.

```python
@dataclasses.dataclass(frozen=True)
class Migration:
    version: int
    rename_collections: Sequence[tuple[str, str]] = ()
    rename_fields: Sequence[tuple[str, str, str]] = ()
    delete_collections: Sequence[str] = ()
    replace_fields: Sequence[tuple[str, str]] = ()
    run: Callable[[Migrating], None] | Callable[[AsyncMigrating], Awaitable[None]] | None = None
```

The `migrations` option of [`Database.open`](./database.md#open) takes a list of them. Opening a file that holds an older schema version runs every version step up to the declared one, in version order, in one write transaction. If any part fails, the file keeps its old schema and data.

The engine makes some changes by itself: it creates new collections, adds new fields that are optional or have a default, builds new indexes and drops removed ones, and retires removed fields. A migration names the rest: a renamed collection or field, a deleted collection, and a field whose type changed. A collection that is gone or a field whose type changed, left unnamed, fails with `INVALID_ARGUMENT`, and so does a name the schema before the step does not have. A field renamed without `rename_fields` is taken as a removed field and a new one, so its values do not follow it to the new name. Every name in a step is the name in the file before that step, so a field of a collection the step renames is named under the collection's old name, and a field declared with `field(name=...)` by the name it has in the file.

This migration takes a file from version 1, where `people` held `fullName` and a string `age`, to version 2:

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

## Fields

### version

```python
version: int
```

The schema version this step leads to: a whole number from 2 up to the declared schema's version. Any other number, or two migrations to the same version, fails with `INVALID_ARGUMENT`, and so do migrations given without a schema.

### rename_collections

```python
rename_collections: Sequence[tuple[str, str]] = ()
```

Pairs of a collection's old name and its new one. The collection keeps its objects where they are, so a rename copies nothing, however many objects the collection holds. A new name that another collection has fails with `INVALID_ARGUMENT`.

### rename_fields

```python
rename_fields: Sequence[tuple[str, str, str]] = ()
```

The collection, by its name before the step, then the field's old name and its new one. No object is rewritten, since a record holds a field's id rather than its name.

### delete_collections

```python
delete_collections: Sequence[str] = ()
```

Collections that go, with every object in them. A collection the new schema leaves out has to be named here. The objects go last, after `run`, which can still read them with `previous`.

### replace_fields

```python
replace_fields: Sequence[tuple[str, str]] = ()
```

Pairs of a collection, by its name before the step, and a field replaced by a new field of the same name, as when a field's type changes. A replaced field is a removed field and a new one, so it holds its default, or `None`, until `run` gives it a value, and a new field that is required needs a default. The old values stay readable in `run` through `previous`. A primary key cannot be replaced.

### run

```python
run: Callable[[Migrating], None] | Callable[[AsyncMigrating], Awaitable[None]] | None = None
```

A function that runs in the migration's write transaction, after the step's renames and the engine's own changes. [Migrating](./migrating.md) gives it the collections of the new schema, and the objects as the old schema read them through `previous` and `previous_keys`. Read an object that way before writing it: a written object keeps only the new schema's fields. One function runs for each version step that has one, in version order.

- Through `Database.open` it is a plain function. A coroutine function fails with `INVALID_ARGUMENT`, whose message names `open_async`.
- Through `Database.open_async` it may be a coroutine function, which receives an [AsyncMigrating](./migrating.md#asyncmigrating); a plain function works there too.
- If it raises, the migration is abandoned, the file keeps its old schema and data, and `open` raises the same error.
