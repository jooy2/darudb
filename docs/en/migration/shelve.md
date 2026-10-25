---
title: shelve
order: 10
languages: [python]
---

# Migrating from shelve

This page moves the data of a `shelve` file into DaruDB: how a shelf's keys and pickled values map onto collections and objects, a script that copies every entry, and what changes in the application.

## How the two map

| `shelve` | DaruDB |
| --- | --- |
| A shelf: one file or a few, depending on the `dbm` module behind it | A database: one file |
| Keys of one kind, such as those that begin with `user:` | A collection |
| A key | The primary key, of type integer, string or bytes; a prefix such as `user:` is dropped |
| A pickled value: a dictionary, or an instance of the application's class | An object, with a field for each of its keys or attributes |
| A value that holds the key of another entry | A link, which holds that object's key |
| `shelf[key] = value` | `put` in a write transaction |
| `sync`, `close` | A commit, which writes only what changed |
| `writeback=True` | No counterpart: change an object with `update` or `put` |

- **Fields get types.** A pickle can hold anything, and two values under keys of one kind can disagree; a DaruDB field holds values of its own type only. Look over the shelf for the strays before copying, and convert them in the script, or the copy stops with `INVALID_ARGUMENT` at the first one.
- **The script needs what the pickles need.** Unpickling a value imports the class it was pickled from, so a shelf of the application's own objects is read where the application's modules can be imported, with the classes as they were when the values were written.
- **The same `dbm` module.** A shelf is a `dbm` file, and which module wrote it depends on the platform and the Python version: `dbm.sqlite3` from Python 3.13, `dbm.gnu` or `dbm.ndbm` where they are built, or `dbm.dumb`. `dbm.whichdb("app")` says which, and the script runs on a Python that has it.
- **Indexes are new.** A shelf is read by key only, so an application that looked values up by a field walked every key or kept a dictionary of its own. Declare an index on each field the application looks up by, and the engine reads only the objects that match.

## Declare the schema

The example moves a shelf of two kinds of key, as an application would have written it: `user:<id>`, whose value is a dictionary of `name`, `email` and `age`, and `post:<slug>`, whose value is a dictionary of `author_id`, the id of a user, `body`, `published_at` and a list of `tags`.

`users` keeps its id as the automatic `id`, and `author_id` becomes a link:

```python
import darudb
from darudb import field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


@darudb.collection("posts")
class Post:
    slug: str = field(primary_key=True)
    author: int | None = field(default=None, link=User, index=True)
    body: str
    published_at: str | None = None
    tags: list[str] = field(index=True)


app = darudb.Schema(1, [User, Post])
```

## Copy the entries

The script reads the shelf with `shelve` itself:

```python
import shelve

import darudb

from schema import Post, User, app

old = shelve.open("app", flag="r")
db = darudb.Database.open("app.darudb", schema=app)


def copy(objects, cls):
    """Copies ``objects`` into the collection of ``cls``, a thousand to a transaction."""
    batch = []

    def flush():
        with db.write(durability="deferred") as txn:
            txn.collection(cls).insert_many(batch)

        batch.clear()

    for obj in objects:
        batch.append(obj)

        if len(batch) == 1000:
            flush()

    if batch:
        flush()


def users():
    for key in old:
        if key.startswith("user:"):
            value = old[key]
            yield User(id=int(key[5:]), name=value["name"], email=value["email"], age=value["age"])


def posts():
    for key in old:
        if key.startswith("post:"):
            value = old[key]
            yield Post(
                slug=key[5:],
                author=value["author_id"],
                body=value["body"],
                published_at=value["published_at"],
                tags=value["tags"],
            )


copy(users(), User)
copy(posts(), Post)

# Every deferred commit is durable once this returns.
db.close()
old.close()
```

`flag="r"` opens the shelf read-only, so it stays as it was. A value that is an instance of the application's class is read the same way, by its attributes, `value.name` in place of `value["name"]`.

## Check the copy

Count the keys of each kind, `sum(1 for key in old if key.startswith("user:"))`, and compare them with `count` on each collection. Then run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

A shelf is read by key; what the application did beyond that becomes a query:

| What the application did | DaruDB |
| --- | --- |
| `old[f"user:{id}"]` | `get(id)` on `users` |
| Walked every `user:` key for the one with an email | `find_one(F.email == email)`, through the unique index |
| Walked every `user:` key, testing the age | `find(F.age.between(18, 30))` |
| Walked every `post:` key for the posts with a tag | `find(F.tags.contains("news"))` on `posts` |
| Kept a dictionary of each user's posts | `find(F.author == id)` on `posts`, through the index |

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **Writes are transactions.** `with db.write() as txn:` commits when the block ends and aborts when it raises, so several changes land together or not at all, and a crash leaves the file as the last commit left it. See [Transactions](../guide/transactions.md).
- **Objects are instances of the collection's class**, frozen dataclasses: a change is made with `update(key, age=31)` of the fields it sets, or `put` of an object made with `dataclasses.replace`, rather than through `writeback`.
- **No pickles.** The file holds records the engine reads without importing any of the application's code, so renaming or moving a class no longer breaks reading old data, and an attribute renamed with `field(name="...")` keeps the name the file stores.
- **Several processes and threads** can use one file at the same time: one writer, any number of readers. See [Several processes](../guide/processes.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
