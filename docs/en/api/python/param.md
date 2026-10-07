---
title: param
order: 12
---

# param

`param` puts a parameter in place of a value in a built query, so that each run of the query gives the value.

```python
def param(index: int) -> Param: ...


class Param:
    index: int

    def __init__(self, index: int) -> None: ...
```

`param(0)` is the first parameter, the same as `$0` in the query language, and `param(1)` the second. `index` is a whole number from 0 to 65535, and anything else fails with `INVALID_QUERY`. A parameter can stand for any value a condition compares with: the value of a comparison, either end of `between`, an element of `is_in`, or the value of `contains`, `startswith` and `endswith`. It cannot stand for a field, a sort, a limit or an offset.

Each `find`, `find_one` or `count` of the query gives the values after the query, in order. A parameter left without a value, or a value of another type than the field's, fails with `INVALID_QUERY`. `None` for a parameter of `==` or `!=` tests whether the field is `None`, as `None` written in the query would; anywhere else it fails with `INVALID_QUERY`.

A query with parameters is usually prepared with [Database.prepare](./database.md#prepare), which compiles it once for its collection and gives a [Prepared](../../types/python/prepared.md) query typed by the class. A built query also runs with parameters as it is, since a query keeps what it compiled for each class and schema it ran with. A prepared query runs only on the collection it was prepared on, and in any transaction, synchronous or asynchronous, read or write. The engine still plans each run for the values it is given.

```python
from darudb import F, param, where

in_ages = db.prepare(User, where(F.age.between(param(0), param(1))).sort_by(F.age))
by_email = db.prepare(User, F.email == param(0))

with db.read() as txn:
    users = txn.collection(User)

    users.find(in_ages, 18, 30)
    users.find_one(by_email, "alice@example.com")
    users.count(by_email, None)  # the users without an email
```

## Param

```python
class Param:
    index: int

    def __init__(self, index: int) -> None: ...
```

What `param` returns, a class of the native module. Two parameters with the same number are equal and hash the same, and the `repr` of one is `param(0)`.

### index

```python
index: int
```

The parameter's number, counted from 0. It is read only.
