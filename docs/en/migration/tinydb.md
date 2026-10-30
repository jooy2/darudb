---
title: TinyDB
order: 9
languages: [python]
---

# Migrating from TinyDB

This page moves the data of a TinyDB file into DaruDB: how tables and documents map onto collections and objects, a script that copies every document, and how the application's queries change.

## How the two map

| TinyDB | DaruDB |
| --- | --- |
| A database: one JSON file | A database: one file |
| A table | A collection |
| A document | An object |
| A document's `doc_id` | The automatic `id`, an integer |
| A field holding a `doc_id` of another table | A link, which holds that object's `id` |
| A string, a number, `true` or `false`, `null` | A string, integer or float, boolean field; `null` makes the field optional |
| A list | A list of values, or of links |
| A nested dictionary | An embedded object, when its keys are fixed |
| A field some documents lack | An optional field, or a field with a default |
| `Query()` | A query, built from `F` or written in the query language |
| A `CachingMiddleware` | Nothing to set: the page cache keeps what is read often |

- **Fields get types.** A TinyDB document can hold any JSON, and two documents of one table can disagree; a DaruDB field holds values of its own type only. Look over each table for the strays before copying, and convert them in the script, or the copy stops with `INVALID_ARGUMENT` at the first one.
- **Every field is declared.** A field that some documents lack becomes optional, or gets a default, and a field that only a few documents have is declared all the same.
- **Keys carry over.** Copying each `doc_id` as the object's `id` keeps every reference between documents valid as a link, and the collection goes on numbering after the largest `id` copied into it.
- **Indexes are new.** A TinyDB query reads every document of its table. Declare an index on each field the application queries by, and the engine reads only the objects that match.

## Declare the schema

The example moves a file of two tables, as an application would have written them: `users`, with `name`, `email` and `age`, and `posts`, with `slug`, `author`, the `doc_id` of a user, `body`, `published_at` and a list of `tags`.

`users` keeps its `doc_id` as the automatic `id`, and `author` becomes a link:

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

The posts take their `slug` as their primary key, since nothing refers to a post by its `doc_id`; a table whose documents are referred to keeps its `doc_id` as `id`, as `users` does.

## Copy the documents

The script reads the file with TinyDB itself:

```python
import darudb
from tinydb import TinyDB

from schema import Post, User, app

old = TinyDB("app.json", access_mode="r")
db = darudb.Database.open("app.darudb", schema=app)


def copy(documents, cls, convert):
    """Copies ``documents`` into the collection of ``cls``, a thousand to a transaction."""
    batch = []

    def flush():
        with db.write(durability="deferred") as txn:
            txn.collection(cls).insert_many(batch)

        batch.clear()

    for document in documents:
        batch.append(convert(document))

        if len(batch) == 1000:
            flush()

    if batch:
        flush()


copy(
    old.table("users"),
    User,
    lambda doc: User(id=doc.doc_id, name=doc["name"], email=doc.get("email"), age=doc.get("age", 0)),
)

copy(
    old.table("posts"),
    Post,
    lambda doc: Post(
        slug=doc["slug"],
        author=doc.get("author"),
        body=doc["body"],
        published_at=doc.get("published_at"),
        tags=doc.get("tags", []),
    ),
)

# Every deferred commit is durable once this returns.
db.close()
old.close()
```

`access_mode="r"` opens the file read-only, so it stays as it was. TinyDB reads the whole file into memory to open a table, so the copy needs memory for it, as the application did. `doc.get` with a default is how a document that lacks a field gets the field's default.

## Check the copy

Compare `len(old.table("users"))` with `count` on `users`, for every table, and run the [integrity check](../guide/tools.md#check-a-file) on the new file, which also checks every object against its indexes.

## Queries

A TinyDB query, here with `User = Query()` and `Post = Query()`, becomes a query built from `F`, much as it was written, or the same in the query language:

| TinyDB                                | DaruDB builder            | DaruDB query language        |
| ------------------------------------- | ------------------------- | ---------------------------- |
| `User.age >= 18`                      | `F.age >= 18`             | `age >= 18`                  |
| `(User.age >= 18) & (User.age <= 30)` | `F.age.between(18, 30)`   | `age BETWEEN 18 AND 30`      |
| `User.email == None`                  | `F.email.is_null()`       | `email IS NULL`              |
| `User.name.matches("A.*")`            | `F.name.startswith("A")`  | `name STARTSWITH "A"`        |
| `User.name.search("li")`              | `F.name.contains("li")`   | `name CONTAINS "li"`         |
| `Post.tags.any(["news"])`             | `F.tags.contains("news")` | `tags CONTAINS "news"`       |
| `table.get(doc_id=7)`                 | `get(7)`                  |                              |
| `table.count(q)`                      | `count(q)`                | `count`, with the same query |

- `matches` and `search` take regular expressions, which have no counterpart: `STARTSWITH`, `ENDSWITH` and `CONTAINS` cover the common ones, and `ignore_case()` makes them, or `==`, ignore case: `F.name.contains("li").ignore_case()`.
- A query sorts and limits in the engine, `where(F.age >= 18).sort_by(F.age, descending=True).limit(10)`, where with TinyDB the application sorted the list `search` returned.
- A test through a link reads the linked object: `F.author.name == "Alice"` on `posts`.

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **Writes are transactions.** `with db.write() as txn:` commits when the block ends and aborts when it raises, so several changes land together or not at all. See [Transactions](../guide/transactions.md).
- **Objects are instances of the collection's class**, frozen dataclasses: a change is made with `update(key, age=31)` of the fields it sets, or `put` of an object made with `dataclasses.replace`.
- **The file is not rewritten whole** on every write: a commit writes only the pages it changed, so writes stay fast as the data grows.
- **Several processes** can use one file at the same time: one writer, any number of readers. See [Several processes](../guide/processes.md).
- **Schema changes** raise the schema's version, and the engine adds new collections, fields and indexes by itself. See [Migrations](../guide/migrations.md).
