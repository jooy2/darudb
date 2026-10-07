---
title: Query
order: 10
counterpart: /api/dart/query-builder
---

# Query

A `Query` says which objects a collection's `find`, `find_one` and `count` look for, in what order, and how many.

```python
class Query:
    def __init__(self) -> None: ...
    def where(self, condition: Condition) -> Query: ...
    def sort_by(self, field: str | FieldRef, *, descending: bool = False) -> Query: ...
    def offset(self, count: int) -> Query: ...
    def limit(self, count: int) -> Query: ...


def where(condition: Condition) -> Query: ...
```

A query has a filter, a sort, an offset and a limit, each optional, and `Query()` has none of them, so it finds every object. The function `where(condition)` is `Query().where(condition)`, the usual way to start one. Each method returns a new query and leaves the one it was called on as it was, so a query can be kept, shared and extended into others. Without a sort, objects come in primary key order, and objects that sort equal come in primary key order too.

A query names fields by their Python attributes, through [F](./conditions.md), and the package gives the engine the names the file stores. It is compiled the first time it runs with a collection's class, and keeps what it compiled for each class and schema, so a query kept and run again is not compiled again, and one run with an old schema's class and a new one's is compiled for each. A query that does not fit the schema, such as one naming a field the collection does not have, fails with `INVALID_QUERY` when it runs.

```python
from darudb import F, where

adults = where(F.age >= 18)
page = adults.sort_by(F.name).offset(20).limit(10)

with db.read() as txn:
    users = txn.collection(User)

    users.count(adults)
    users.find(page)
```

A condition on the primary key or on an indexed field, joined to the rest of the filter with `&`, lets the engine read only the objects that meet it, and a sort by one indexed field alone reads in that order and stops at the limit. The result is the same either way. [Queries](../../guide/queries.md) has the longer explanation.

## Methods

### where

```python
def where(self, condition: Condition) -> Query: ...
```

Keeps the objects that meet `condition`, a [Condition](./conditions.md) made from `F`, and every condition given to `where` before: each call is joined to the earlier ones with AND. Anything that is not a condition fails with `INVALID_QUERY`.

```python
from darudb import F, where

query = where(F.age.between(18, 30)).where(F.tags.contains("new") | F.email.is_null())
```

### sort_by

```python
def sort_by(self, field: str | FieldRef, *, descending: bool = False) -> Query: ...
```

Sorts by `field`, ascending unless `descending`, after any sort given before. The field is an `F` path, as `F.address.city`, or the same path as a string of attributes separated by dots, as `"address.city"`. `None` sorts first ascending and last descending, and strings sort by their UTF-8 bytes. Sorting by a list, by an embedded object, or through a to-many link fails with `INVALID_QUERY` when the query runs.

### offset

```python
def offset(self, count: int) -> Query: ...
```

Skips the first `count` objects of the sorted result. A later call replaces an earlier one. A negative `count` fails with `INVALID_QUERY` at once.

### limit

```python
def limit(self, count: int) -> Query: ...
```

Returns at most `count` objects. A later call replaces an earlier one, and `count` follows the rule of `offset`.

## The query language

`find`, `find_one`, `count` and `Database.prepare` also take a query as text, which the engine parses into the same query a builder makes. The collection is not part of the text: the call that runs it names it. The text names fields as the file stores them, so a field declared with `field(name=...)` goes by its stored name there.

```python
with db.read() as txn:
    txn.collection(User).find(
        'age >= $0 AND (name STARTSWITH "A" OR email IS NULL) SORT BY age DESC LIMIT 10', 18
    )
```

- **Order.** A filter comes first, then `SORT BY` with fields separated by commas, each ascending unless `DESC` follows it, then `LIMIT` and `OFFSET`. Each part may be left out.
- **Conditions.** `field == value`, with `!=`, `<`, `<=`, `>` and `>=` too; `field BETWEEN a AND b`; `field IN [a, b]`; `field CONTAINS value`, `STARTSWITH` and `ENDSWITH`; `field IS NULL` and `field IS NOT NULL`. They join with `AND`, `OR`, `NOT` and parentheses, and `AND` binds tighter than `OR`. A field may be a dotted path.
- **Values.** An int has no point, and a float has one or an exponent; either may start with `-`. Strings are in double quotes, with the escapes `\"`, `\\`, `\n`, `\t` and `\u{...}`. The other values are `true`, `false` and `null`.
- **Parameters.** `$0`, `$1` and on take the values passed after the text, in order. A value that comes from outside the program belongs in a parameter, never in the text.
- **Names.** Keywords are case-insensitive. A field named like a keyword, such as `limit`, goes in backticks where a path starts.
- **Errors.** Text that does not parse fails with `INVALID_QUERY`, and the message names the character, counted from 1, where it went wrong. Parentheses and `NOT` nest at most 48 levels deep.

The package keeps the last 256 texts it has parsed, so a text that runs again is not parsed again.
