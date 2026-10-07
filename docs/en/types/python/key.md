---
title: Key
order: 2
---

# Key

`Key` is a primary key as Python passes and receives it: an `int`, a `str` or `bytes`.

```python
Key: TypeAlias = int | str | bytes
```

`get`, `update` and `delete` take a key, `insert`, `insert_many`, `put` and `put_many` return the keys of the objects they write, and `previous_keys` returns keys too. A link field holds the key of the object it links to. Which of the three a collection's keys are follows from its key field, which [`field(primary_key=True)`](../../api/python/collection.md#primary-keys) marks:

| Key field               | Passed as                            | Returned as |
| ----------------------- | ------------------------------------ | ----------- |
| None, the engine's `id` | `int`                                | `int`       |
| An `int` field          | `int`                                | `int`       |
| A `str` field           | `str`                                | `str`       |
| A `bytes` field         | `bytes`, `bytearray` or `memoryview` | `bytes`     |

- **Ints** are 64-bit: from -2^63 to 2^63 - 1. One outside that range fails with `INVALID_ARGUMENT`. A `bool` is not a key, although Python counts it as an `int`.
- **Strings** are passed and returned as `str`.
- **Bytes** may be passed in any of the three types Python keeps them in, and come back as `bytes`. `Key` names only `bytes`, so a type checker asks for a `bytearray` or a `memoryview` to be converted first.

A key of another type than the collection's key field fails with `INVALID_ARGUMENT`, and so do `None`, a `float` and any other value. A string or bytes key also has to fit in a key of the file: at most 957 bytes once encoded, with 4096-byte pages, which is a few bytes more than the key. A longer key fails with `INVALID_ARGUMENT` when the object is written.

```python
import darudb
from darudb import field


@darudb.collection("files")
class File:
    digest: bytes = field(primary_key=True)
    size: int


with darudb.Database.open("files.darudb", schema=darudb.Schema(1, [File])) as db:
    with db.write() as txn:
        files = txn.collection(File)
        key = files.insert(File(digest=b"\xca\xfe", size=2))  # b"\xca\xfe"

        files.get(key)
```
