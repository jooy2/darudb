---
title: Prepared
order: 6
---

# Prepared

`Prepared` is a query compiled once, on one collection, that each run gives values for its parameters.

```python
class Prepared(Generic[T]):
    collection: str
```

[`Database.prepare`](../../api/python/database.md#prepare) makes one, from text in the query language with `$0`, `$1` and on, or from a [Query](../../api/python/query.md) or a [condition](../../api/python/conditions.md) built with [`param`](../../api/python/param.md) in place of the values that change. `find`, `find_one` and `count` take it with the values of its parameters after it. `T` is the collection's class, so a type checker knows the objects a prepared query finds.

A prepared query holds no database and no transaction, so it runs in any transaction, synchronous or asynchronous, read or write. Preparing text saves parsing it on every run. The package also keeps the last 256 texts passed straight to `find`, `find_one` or `count` parsed, so preparing text matters most to a program that runs more texts than that. A built query keeps what it compiled for each class and schema whether or not it is prepared, so preparing one checks its collection once, ahead, and gives a query typed by the class. The engine still plans each run for the values it is given.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))
in_ages = db.prepare(User, "age BETWEEN $0 AND $1 SORT BY age")

with db.read() as txn:
    users = txn.collection(User)

    users.find_one(by_email, "alice@example.com")
    users.find(in_ages, 18, 30)
```

- `prepare` fails with `INVALID_ARGUMENT` for a collection the schema does not have, with `INVALID_QUERY` for text that does not parse, and with `CLOSED` after the database is closed.
- A run fails with `INVALID_QUERY` on another collection, when it leaves a parameter without a value, and when a field or a value of the query does not fit the schema.

## Properties

### collection

```python
collection: str
```

The name of the collection the query was prepared on, and the only one it runs on. `repr` of a prepared query shows it, as `Prepared('users')`.
