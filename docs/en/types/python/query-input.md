---
title: QueryInput
order: 7
group: queries
pageClass: reference-page
---

# QueryInput

`QueryInput` is what `find`, `find_one` and `count` take as their query: a built query, a condition, text, a prepared query, or nothing.

```python
QueryInput: TypeAlias = "Query | Condition | str | Prepared[Any] | None"
```

[ReadCollection](../../api/python/read-collection.md) takes it as its first argument, which is positional only, and the values of the query's parameters after it.

- **`None`**, or no argument: `find()` returns every object, in primary key order, and `count()` counts them.
- **A [Query](../../api/python/query.md)**, as `where(F.age >= 18).sort_by(F.age)`, which can be built once and passed many times.
- **A [Condition](../../api/python/conditions.md)**, as `F.age >= 18`, which stands for the query with that condition alone.
- **A `str`** in the [query language](../../api/python/query.md#the-query-language), with `$0`, `$1` and on for its parameters.
- **A [Prepared](./prepared.md) query** that `Database.prepare` made on the same collection.

Anything else fails with `INVALID_QUERY`. [Queries](../../guide/queries.md) explains what a query can say.

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    users.find(where(F.age >= 18).sort_by(F.age, descending=True).limit(10))
    users.count(F.email.is_null())
    users.find("age >= $0 AND name STARTSWITH $1", 18, "A")
```

## Parameters

```python
*parameters: object
```

The values of a query's parameters, in order, as positional arguments after the query: `$0` or `param(0)` first. Each is one value a condition compares with, whose Python type decides its type in the engine:

- A `bool` is a bool, and an `int` is an int, which has to fit in 64 bits.
- A `float` is a float. An `int` field compares only with an int, and a `float` field with any number.
- A `str` is a string, and `bytes`, a `bytearray` or a `memoryview` is bytes.
- `None` is allowed only where the parameter is compared with `==` or `!=`, which then test whether the field is `None`. Anywhere else it fails with `INVALID_QUERY`.

A run that leaves a parameter without a value fails with `INVALID_QUERY`, and so does a value of another type than its field's, or one that is not a single value, such as a list. Values given to a query without parameters are ignored.

```python
users.find_one("email == $0", None)  # a user without an email
```

A value that comes from outside the program belongs in a parameter, never in the text.
