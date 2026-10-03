---
title: Error
order: 15
counterpart: /types/rust/error
---

# Error

Every error the package throws is a JavaScript `Error` with a `code`, one of the engine's error codes, beside its `message`.

```ts
interface CodeError extends Error {
  code: string;
}
```

That is how the package's own code declares what it throws, but it exports no such type and no error class: what it throws is a plain `Error`, `instanceof Error` included, with a `code` property of its own. In strict TypeScript a caught value is `unknown`, so check for the property before reading it. [Errors](../../guide/errors.md) lists every code, when it happens and what to do about it.

```ts
import { Database } from 'darudb';

try {
  Database.open('app.darudb', { create: false });
} catch (error) {
  if (error instanceof Error && 'code' in error && error.code === 'NOT_FOUND') {
    // Nothing exists at that path.
  } else {
    throw error;
  }
}
```

## Properties

### code

```ts
code: string;
```

The failure's code in `SCREAMING_SNAKE_CASE`, such as `DUPLICATE_KEY`. It is the same string in every language, and a code is never renamed once released, so a program can rely on it.

### message

```ts
message: string;
```

What went wrong, for a person to read. A message may be reworded in any release, so compare the `code`, never the message.

## Where errors come from

- **The engine.** A failure inside the engine arrives with the engine's code and message unchanged, whether the call ran on the calling thread or on the thread pool.
- **The package's checks.** The package checks what it is given before it reaches the engine, and fails with the engine's codes: `INVALID_ARGUMENT` for an option, an object or a key that does not fit, `INVALID_QUERY` for a query or parameters that do not, `CORRUPTED` for a record read from the file that does not decode, and `INTERNAL` for what only a bug can cause.
- **Your functions.** An error that a transaction's function or a migration function throws comes out of `read`, `write`, `open` and their `Async` twins as it was thrown, once the transaction has ended without committing. It has whatever `code` it had, often none.

## CLOSED

- After `close` or `closeAsync`, every member of a [Database](../../api/node/database.md) but `path`, `isOpen`, `close` and `closeAsync` throws `CLOSED`, and the asynchronous methods reject with it. Closing again does nothing. `closeAsync` refuses new work as soon as it is called, before its promise settles.
- A transaction, or a collection taken from it, used after its function has returned throws `CLOSED`, and in the asynchronous API rejects with it. Keep the objects a transaction read instead: they are plain values that outlive it.

## Asynchronous calls

A method that returns a promise never throws: when it fails, even for an argument it refuses, its promise rejects with the same kind of `Error` the synchronous form throws. In an asynchronous transaction, a refused operation rejects its own promise and changes nothing, and the operations called after it still run. If the transaction's function lets the rejection through, the transaction aborts, and `writeAsync` rejects with that error.

```ts
try {
  await db.writeAsync(async (txn) => {
    await txn.collection('users').insert({ name: 'Alice', email: 'alice@example.com' });
  });
} catch (error) {
  if (error instanceof Error && 'code' in error && error.code === 'DUPLICATE_KEY') {
    // The email is taken; nothing was committed.
  } else {
    throw error;
  }
}
```
