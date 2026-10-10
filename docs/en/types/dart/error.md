---
title: DaruException
order: 14
group: errors
counterpart: /types/rust/error
pageClass: reference-page
---

# DaruException

`DaruException` is what the package throws for every failure of the database, with one of the engine's error codes beside its message.

```dart
final class DaruException implements Exception {
  const DaruException(this.code, this.message);

  final String code;
  final String message;
}
```

[Errors](../../guide/errors.md) lists every code, when it happens and what to do about it.

```dart
try {
  Database.open('app.darudb', create: false);
} on DaruException catch (error) {
  if (error.code != 'NOT_FOUND') {
    rethrow;
  }

  // Nothing exists at that path.
}
```

## Properties

### code

```dart
final String code;
```

The failure's code in `SCREAMING_SNAKE_CASE`, such as `DUPLICATE_KEY`. It is the same string in every language, and a code is never renamed once released, so a program can rely on it.

### message

```dart
final String message;
```

What went wrong, for a person to read. A message may be reworded in any release, so compare the `code`, never the message.

## Where errors come from

- **The engine.** A failure inside the engine arrives with the engine's code and message unchanged, whether the call ran on the calling isolate or on a thread of the native library. A panic in the native library, which only a bug can cause, arrives as `INTERNAL`.
- **The package's checks.** The package checks what it is given before it reaches the engine, and fails with the engine's codes: `INVALID_ARGUMENT` for an option, an object or a key that does not fit, `INVALID_QUERY` for a query or parameters that do not, and `CORRUPTED` for a record read from the file that does not decode.
- **Your functions.** An error that a transaction's function or a migration function throws comes out of `read`, `write`, `open` and their `Async` twins as it was thrown, once the transaction has ended without committing.

## CLOSED

- After `close` or `closeAsync`, every member of a [Database](../../api/dart/database.md) but `path`, `isOpen`, `close` and `closeAsync` throws `CLOSED`, and the asynchronous methods fail with it. Closing again does nothing.
- A transaction, or a collection taken from it, used after its function has returned throws `CLOSED`, and in the `Future` API fails with it. Keep the objects a transaction read instead: they are values that outlive it.

## The Future API

A method that returns a `Future` never throws: when it fails, even for an argument it refuses, its `Future` completes with the same `DaruException` the synchronous form throws. In an asynchronous transaction, a refused call fails its own `Future` and changes nothing, and the calls made after it still run. If the transaction's function lets the error through, the transaction aborts, and `writeAsync` fails with that error.

```dart
try {
  await db.writeAsync((txn) async {
    await txn.collection(userSchema).insert(const User(name: 'Alice', email: 'alice@example.com'));
  });
} on DaruException catch (error) {
  if (error.code != 'DUPLICATE_KEY') {
    rethrow;
  }

  // The email is taken; nothing was committed.
}
```
