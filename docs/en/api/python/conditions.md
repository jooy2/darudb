---
title: conditions
order: 11
counterpart: [/api/rust/filter, /api/dart/fields]
---

# conditions

`F` names the fields of the objects a query tests, and comparing a field makes a `Condition`, which joins others with `&`, `|` and `~`.

```python
F: Final[_Fields]


class FieldRef:
    def __getattr__(self, name: str) -> FieldRef: ...
    def __getitem__(self, name: str) -> FieldRef: ...


class Condition:
    def __and__(self, other: Condition) -> Condition: ...
    def __or__(self, other: Condition) -> Condition: ...
    def __invert__(self) -> Condition: ...
    def query(self) -> Query: ...
```

`F.age` is the field `age`, a `FieldRef`, and `F.age >= 18` is a condition on it. [`where`](./query.md#where) takes a condition, and so do `find`, `find_one` and `count`, which take one as the query with that condition alone. A condition belongs to no collection: it is checked against the collection's schema when the query runs, and a field the collection does not have, or a value of another type, fails with `INVALID_QUERY` then.

```python
from darudb import F

young = (F.age >= 18) & (F.age < 30)

with db.read() as txn:
    found = txn.collection(User).find(young | F.email.is_null())
```

Python's `&`, `|` and `~` bind tighter than comparisons, so each comparison they join goes in parentheses: `(F.age >= 18) & (F.age < 30)`.

- **Paths.** A field is named, or reached through embedded objects and links: `F.address.city`, or `F.team.city` to test the linked object. A link to an object that is not there reads as `None`. A path has at most 32 names.
- **Attributes.** A path names each field by its Python attribute, and the package gives the engine the name the file stores, through embedded classes and linked collections too, so a field declared with `field(name=...)` keeps its attribute in queries. A name that is no attribute of the class goes to the engine as it is.
- **Lists.** A condition on a list holds when it holds for any element, and `contains` on a list looks for an element. An empty list has no element, so only `is_not_null` holds for it.
- **None.** Every condition on a field that is `None` is false, except `is_null`. `F.email == None` is `F.email.is_null()`, and `F.email != None` is `F.email.is_not_null()`. Any other test with `None` fails with `INVALID_QUERY` at once. An embedded object itself is never tested, not even for `None`: a condition tests one of its fields.
- **Types.** A value has the field's type. An `int` field compares with an `int` and never a `float` or a `bool`, a `float` field with any number, and a link with the linked collection's key. A value is one `bool`, `int`, `float`, `str` or bytes, as `bytes`, `bytearray` or `memoryview`, or a [param](./param.md); a list, or anything else, fails with `INVALID_QUERY` when the query runs.
- **Nesting.** A filter nests at most 24 levels deep. An `&` inside an `&`, and an `|` inside an `|`, become one level, so only `~` and alternating groups count.

## F

```python
F: Final[_Fields]
```

The root of every field's path. An attribute of `F` is the field of that name, and an attribute of a field is a field inside it: `F.address.city`. `F["name"]` does the same, for a name that is also an attribute of `FieldRef` itself, as the methods below are, or that begins with two underscores: `F.address["contains"]` is the field `contains` of `address`.

## FieldRef

```python
class FieldRef:
    def __getattr__(self, name: str) -> FieldRef: ...
    def __getitem__(self, name: str) -> FieldRef: ...
```

A field of the objects a query tests, by its path, which `F` makes. Its comparisons make conditions rather than `bool` values, so a `FieldRef` is not hashable and cannot be a key of a `dict` or a member of a `set`. Its `repr` is its path, as `F.address.city`.

### Comparisons

```python
def __eq__(self, value: object) -> Condition: ...
def __ne__(self, value: object) -> Condition: ...
def __lt__(self, value: object) -> Condition: ...
def __le__(self, value: object) -> Condition: ...
def __gt__(self, value: object) -> Condition: ...
def __ge__(self, value: object) -> Condition: ...
```

`==`, `!=`, `<`, `<=`, `>` and `>=` with a value make a condition: the field equals the value, differs from it, is less than it, and so on. The field may be on either side, so `18 <= F.age` is `F.age >= 18`. A chained comparison such as `18 <= F.age < 30` raises `TypeError`, since Python joins its two halves with `and`; write `(F.age >= 18) & (F.age < 30)` or `F.age.between(18, 29)`.

### between

```python
def between(self, low: object, high: object) -> Condition: ...
```

The field is from `low` to `high`, both included.

### is_in

```python
def is_in(self, values: Iterable[object]) -> Condition: ...
```

The field equals one of `values`, any iterable of them. An empty one matches nothing.

### contains

```python
def contains(self, value: object) -> Condition: ...
```

A string field contains `value`, or a list holds the element `value`.

### startswith

```python
def startswith(self, value: object) -> Condition: ...
```

A string field starts with `value`. On a list of strings, an element does.

### endswith

```python
def endswith(self, value: object) -> Condition: ...
```

A string field ends with `value`. On a list of strings, an element does.

### is_null

```python
def is_null(self) -> Condition: ...
```

The field is `None`. A list is `None` only when the list itself is, never when it is empty.

### is_not_null

```python
def is_not_null(self) -> Condition: ...
```

The field is not `None`.

## Condition

```python
class Condition:
    def __and__(self, other: Condition) -> Condition: ...
    def __or__(self, other: Condition) -> Condition: ...
    def __invert__(self) -> Condition: ...
    def query(self) -> Query: ...
```

A test of one field, or tests joined. A condition never changes once made, so it can go into any number of queries.

A condition has no truth value: `bool` of one raises `TypeError`, and so do `and`, `or`, `not` and `in`, which ask for one. Join conditions with `&`, `|` and `~` instead. `&` or `|` with anything but a condition raises `TypeError` too.

### & (and)

```python
def __and__(self, other: Condition) -> Condition: ...
```

Both conditions hold.

### | (or)

```python
def __or__(self, other: Condition) -> Condition: ...
```

At least one of the conditions holds.

### ~ (not)

```python
def __invert__(self) -> Condition: ...
```

The condition does not hold.

### query

```python
def query(self) -> Query: ...
```

A [Query](./query.md) with this condition alone, made the first time it is asked for and kept, which is the query `find` runs for a condition. Since it is kept, a condition run again is not compiled again.
