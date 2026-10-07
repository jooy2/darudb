---
title: Schema
order: 3
---

# Schema

`Schema` lists a database's collections, as their classes, at a version, for `Database.open` to take.

```python
class Schema:
    version: int
    collections: tuple[type, ...]

    def __init__(self, version: int, collections: Sequence[type]) -> None: ...
```

`version` is a whole number from 1 up, and anything else fails with `INVALID_ARGUMENT`; raise it whenever the schema changes. `collections` lists the classes decorated with [`@collection`](./collection.md), and every collection a link names is among them.

Making a schema reads every class: its annotations, defaults and options, and the embedded classes it holds. Each class is read once in a process, and the schema keeps what the engine needs to open a file with it, so any number of opens can share one schema. A class that breaks a rule of [`@collection`](./collection.md) fails here with `INVALID_ARGUMENT`, and so does a class not decorated with `@collection`, an `@embedded` class among them, or two classes of one collection name. The rest is checked when the database opens, which fails with `INVALID_ARGUMENT` for a schema the engine cannot store, such as a link to a collection the schema does not have, or a default of another type than its field's.

```python
import darudb
from darudb import field


@darudb.embedded
class Address:
    city: str
    zip: str | None = field(default=None, name="postcode")


@darudb.collection("teams")
class Team:
    name: str = field(primary_key=True)
    city: str | None = None


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    tags: list[str] = field(default_factory=list, index=True)
    team: str | None = field(default=None, link=Team)
    address: Address | None = None


app = darudb.Schema(1, [Team, User])
db = darudb.Database.open("app.darudb", schema=app)
```

The examples on the other pages of this section use these classes and this `db`.

The first open stores the schema in the file, and every later open compares the declared schema with the stored one:

- **The same version and the same schema**: nothing to do. Listing classes or declaring fields in another order is not a change, since fields are matched by name.
- **The same version and another schema**: `SCHEMA_MISMATCH`, since the schema changed without a new version.
- **A newer version in the file**: `SCHEMA_TOO_NEW`, since a newer application wrote it.
- **An older version in the file**: a migration, which [Migration](./migration.md) and the guide's [Migrations](../../guide/migrations.md) explain.

## Properties

### version

```python
version: int
```

The schema version.

### collections

```python
collections: tuple[type, ...]
```

The classes, in the order they were given. `repr` of a schema lists them by name, as `Schema(1, [Team, User])`.
