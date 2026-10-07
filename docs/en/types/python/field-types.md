---
title: Field types
order: 3
counterpart: /types/rust/field-type
---

# Field types

A field's annotation decides what the field holds in the file, which Python values it takes, and what an object read from the file holds in it.

| Annotation | In the file | Takes | Reads as |
| --- | --- | --- | --- |
| `bool` | A bool | `bool` | `bool` |
| `int` | An int, 64 bits | `int` from -2^63 to 2^63 - 1, not a `bool` | `int` |
| `float` | A float, 64 bits | `float`, or an `int` | `float` |
| `str` | A string, as UTF-8 | `str` | `str` |
| `bytes` | Bytes | `bytes`, `bytearray` or `memoryview` | `bytes` |
| `list[E]` of those | A list | Any iterable of `E` but a `str` or bytes | `list` |
| `int`, `str` or `bytes` with `field(link=...)` | A link | The linked object's key | The key |
| `list[int]`, `list[str]` or `list[bytes]` with `field(link=...)` | A list of links | Any iterable of keys | `list` of keys |
| A class decorated with `@embedded` | An embedded object | An instance of the class | An instance of the class |
| Any of these `\| None` | The same, optional | The same, or `None` | The same, or `None` |

A value of another type than its field's fails with `INVALID_ARGUMENT` when the object is written. Any other annotation fails with `INVALID_ARGUMENT` when the [Schema](../../api/python/schema.md) is made: a `dict`, a `list` without its element type, a union of two types, a list of lists or of embedded objects, and a list whose elements may be `None`.

```python
import darudb
from darudb import field


@darudb.embedded
class Address:
    city: str
    zip: str | None = None


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    rating: float = 0.0
    active: bool = True
    tags: list[str] = field(default_factory=list)
    avatar: bytes | None = None
    friends: list[int] = field(default_factory=list, link="users")
    address: Address | None = None
```

## Required, optional and default

- **Required**: a field that is not `| None` and has no default, such as `name: str`. Every object has it, and `None` in it fails with `INVALID_ARGUMENT`.
- **Optional**: a field annotated `X | None`, or `Optional[X]`, such as `email: str | None = None`. It may be `None`, and a record written before the field existed reads it as `None`.
- **Default**: a field that is not `| None` and has a default, such as `age: int = 0`. It is required, and a record written before the field existed reads it as the default. `None` written in it stores the default as well.

A default is a constant the file can store: a `bool`, an `int`, a `float`, a `str`, `bytes`, or a list of them. An `int` default of a `float` field is stored as a `float`. An optional field's only default is `None`.

## Values

- **Ints** are 64-bit. A `bool` is refused in an `int` field and in a `float` field, although Python counts it as an `int`.
- **Floats** take an `int` too, since Python code writes a whole number as an `int` as often as not, and it reads back as a `float`.
- **Strings** are stored as UTF-8 and compared by their bytes.
- **Bytes** come in any of the three types Python keeps them in, and come back as `bytes` of their own.
- **Lists** take any iterable but a `str` and bytes, which would be read as a list of characters or numbers, and come back as a new `list`. A list holds no `None`.
- **Embedded objects** are instances of their class, or of a subclass of it, and come back as instances of the class, built without its `__init__`. A `dict` is refused.
- **Links** hold the key of an object of the linked collection, of that collection's key type, and come back as the key. A link to an object that does not exist is allowed.
