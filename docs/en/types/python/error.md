---
title: DaruError
order: 1
---

# DaruError

`DaruError` is the one exception the package raises for every failure of the database, with one of the engine's error codes beside its message.

```python
class DaruError(Exception):
    code: str
    message: str

    def __init__(self, code: str, message: str) -> None: ...
```

It is a plain `Exception` subclass, imported as `darudb.DaruError`. `str` of one is `CODE: message`, and its `args` are `(code, message)`. [Errors](../../guide/errors.md) lists every code, when it happens and what to do about it.

```python
import darudb

try:
    darudb.Database.open("app.darudb", create=False)
except darudb.DaruError as error:
    if error.code != "NOT_FOUND":
        raise

    # Nothing exists at that path.
```

## Properties

### code

```python
code: str
```

The failure's code in `SCREAMING_SNAKE_CASE`, such as `DUPLICATE_KEY`. It is the same string in every language, and a code is never renamed once released, so a program can rely on it.

### message

```python
message: str
```

What went wrong, for a person to read. A message may be reworded in any release, so compare the `code`, never the message.

## Where errors come from

- **The engine.** A failure inside the engine arrives with the engine's code and message unchanged, whether the call ran on the calling thread or on the package's thread pool.
- **The package's checks.** The package checks what it is given before it reaches the engine, and fails with the engine's codes: `INVALID_ARGUMENT` for an option, a class, an object or a key that does not fit, and `INVALID_QUERY` for a query or parameters that do not. A Python value the engine cannot hold is `INVALID_ARGUMENT` too, rather than a `TypeError`, so that one `except` catches every refusal.
- **Your functions.** An error that a transaction's block or a migration function raises comes out of the `with` statement or `open` as it was raised, once the transaction has ended without committing.

Python itself raises a few errors that are not a `DaruError`: `TypeError` for the truth value of a [condition](../../api/python/conditions.md) and for `Database()`, and the errors of the dataclass a decorated class is, such as `TypeError` for a constructor call that leaves out a required field and `dataclasses.FrozenInstanceError` for assigning to a field.

## CLOSED

- After `close` or `close_async`, every member of a [Database](../../api/python/database.md) that uses the file raises `CLOSED`, the asynchronous ones included. `path`, `schema`, `schema_version` and `is_open` can still be read, and closing again does nothing.
- A transaction, or a collection taken from it, used after its block has ended raises `CLOSED`. Keep the objects a transaction read instead: they are instances of their class that outlive it.

## Asynchronous calls

A coroutine of the package raises its `DaruError` where it is awaited, for an argument it refuses as well as for a failure of the engine. In an asynchronous transaction, a refused operation changes nothing, and the operations after it still run. If the block lets the error through, the transaction aborts, and the error leaves the `async with` statement.

```python
async def main() -> None:
    try:
        async with db.write_async() as txn:
            await txn.collection(User).insert(User(name="Alice", email="alice@example.com"))
    except darudb.DaruError as error:
        if error.code != "DUPLICATE_KEY":
            raise

        # The email is taken; nothing was committed.
```
