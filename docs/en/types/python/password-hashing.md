---
title: PasswordHashing
order: 5
---

# PasswordHashing

`PasswordHashing` is what hashing a password into a key costs, as Argon2id counts it: memory, iterations and parallelism.

```python
@dataclasses.dataclass(frozen=True)
class PasswordHashing:
    memory_kib: int = 19456
    iterations: int = 2
    parallelism: int = 1
```

The `password_hashing` option of [`Database.open`](../../api/python/database.md#open) and of `backup` takes one. More memory and more iterations make guessing a password slower for an attacker, and opening the database slower for everyone. The default takes tens of milliseconds on a current computer and fits the memory limits of a mobile app extension. Each field has its default, so `PasswordHashing(memory_kib=65536)` changes the memory alone.

```python
import darudb
from darudb import PasswordHashing

db = darudb.Database.open(
    "secret.darudb",
    password="correct horse battery staple",
    password_hashing=PasswordHashing(memory_kib=65536, iterations=3),
)
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `memory_kib` | `int` | Memory, in KiB, from 8 per lane of `parallelism` up to 1 GiB (1048576). 19456 by default |
| `iterations` | `int` | Passes over the memory, from 1 to 1024. 2 by default |
| `parallelism` | `int` | Lanes, from 1 to 64. 1 by default |

A field that is not a whole number from 0 up, such as `-1`, `1.5` or `True`, fails with `INVALID_ARGUMENT` when the `PasswordHashing` is made. A value outside its range fails with `INVALID_ARGUMENT` when the option is used, which it is whenever it is given, even when no password is.

## When it applies

- **A new database created with a `password`.** The file records this cost beside the key.
- **`set_password` and `set_password_async`.** The new password is hashed at the cost this option gave when the process opened the file, the first handle's if it opened the file more than once, and the file records that cost from then on. Without the option, that is the default cost, whatever the file recorded before.
- **A backup under a new `password`**, which records the cost of its own `password_hashing` in the copy.
- **Never when a file is opened.** Opening an encrypted file takes the cost the file records, so raising the cost in a new release of an application still opens the files it made before. [`Database.salvage`](../../api/python/database.md#salvage) does the same, and takes no cost.

[Encryption](../../guide/encryption.md) explains how the key derived from a password protects the file.
