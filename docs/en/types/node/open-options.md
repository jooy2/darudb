---
title: OpenOptions
order: 1
counterpart: /api/rust/open-options
---

# OpenOptions

`OpenOptions` is what `Database.open` takes besides the path: whether to create the file, its page size, how long to wait for other processes, the page cache, the schema and its migrations, and the key or password of an encrypted file.

```ts
interface OpenOptions<S = Schema>
```

Every field is optional, and `Database.open(path)` without options creates or opens a plain database without a schema. `S` is the type of the [schema](../../api/node/schema.md), which [`Database.open`](../../api/node/database.md) infers from the `schema` option and passes on to the migration functions. It carries types only.

Some options belong to the file rather than to one `Database`. A process that opens a file it already has open gets another handle to the same database, and `busyTimeout`, `cacheSize` and `passwordHashing` stay those of the first handle. `pageSize` matters only when the file is created. Each handle keeps the `schema` it was opened with, and an encrypted file needs its `key` or `password` on every open.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, { notes: collection({ text: t.string() }) });

const db = Database.open('notes.darudb', {
  schema: app,
  busyTimeout: 2000,
  cacheSize: 8 * 1024 * 1024
});
```

## Fields

### create

```ts
create?: boolean;
```

Whether to create the database when nothing exists at the path. `true` by default. With `false`, opening a path where nothing exists fails with `NOT_FOUND`. An existing file is never replaced either way. A new database is written to a temporary file and then moved into place, so the path holds either nothing or a whole database.

### pageSize

```ts
pageSize?: number;
```

The page size of a new database, in bytes: a power of two from 4096 to 65536, and 4096 by default. Any other value fails with `INVALID_ARGUMENT`, even when the file exists. An existing file keeps the page size its header records, which [`Database.pageSize`](../../api/node/database.md) reads. Larger pages make scans and counts faster, but they make a small commit slower, and a lookup in a file larger than the page cache too, since both write or read whole pages.

### busyTimeout

```ts
busyTimeout?: number;
```

How long, in milliseconds, opening and a write transaction wait for another process's writer before failing with `BUSY`. 5000 by default. [Several processes](../../guide/processes.md) explains how processes take turns at writing.

### cacheSize

```ts
cacheSize?: number;
```

How much memory the page cache may take, in bytes: a whole number from 0 up, and 32 MiB by default. Any other value fails with `INVALID_ARGUMENT`. The cache keeps pages read from the file, already checked and decrypted, so that reading one again costs neither a read nor a check. It holds at least 16 pages whatever this says, and fills only as pages are read, so a database smaller than the cache never takes all of it. A larger cache speeds up a database that does not fit in it; a process short of memory can give it less.

### schema

```ts
schema?: S;
```

The collections the database holds, as [`schema`](../../api/node/schema.md) declares them. The first open stores the schema in the file, and every later open compares the two:

- The same version with a different schema fails with `SCHEMA_MISMATCH`.
- A file holding a newer version fails with `SCHEMA_TOO_NEW`.
- A file holding an older version is migrated before `open` returns, with `migrations`.

A declaration the engine cannot store, such as a link to a collection the schema does not have, fails with `INVALID_ARGUMENT`. Without a schema the database has no collections: `schemaVersion` is `null`, and `collection` fails with `INVALID_ARGUMENT`. [Collections and objects](../../guide/objects.md) has the rules a schema follows.

### migrations

```ts
migrations?: Migration<S>[];
```

How a file holding an older schema version becomes this one: a [Migration](./migration.md) for each version step that needs more than the engine does by itself. They run only when a file needs them, in version order and in one write transaction. Migrations without a `schema`, or a value that is not an array, fail with `INVALID_ARGUMENT`.

### key

```ts
key?: Uint8Array;
```

A key of 32 bytes that encrypts a new database or opens an encrypted one. Another length, or a value that is not a `Uint8Array`, fails with `INVALID_ARGUMENT`.

- An encrypted database opened without a key or password fails with `KEY_REQUIRED`, and with another one with `WRONG_KEY`.
- A plain database opened with a key fails with `INVALID_ARGUMENT`. It never becomes encrypted: that takes a new file.
- Keep the key where it cannot be lost, such as the operating system's keystore. Without it, the data cannot be read.

The package copies the key when `open` is called and wipes its copy once the engine has its own, so you can wipe your buffer with `fill(0)` as soon as the call returns. [Encryption](../../guide/encryption.md) explains how the key protects the file.

### password

```ts
password?: string | Uint8Array;
```

A password that encrypts a new database or opens an encrypted one. It is hashed with Argon2id, at the cost `passwordHashing` sets, into the key that protects the file, and otherwise works as `key` does. A string is used as its UTF-8 bytes. An empty password, or a `key` and a `password` together, fails with `INVALID_ARGUMENT`. A `Uint8Array` can be wiped once `open` returns; a string cannot, and stays in memory until the garbage collector reclaims it.

### passwordHashing

```ts
passwordHashing?: PasswordHashing;
```

How much work hashing a password takes when a new database is encrypted with one or `setPassword` changes it: 19456 KiB, 2 iterations and a parallelism of 1 by default. Opening an existing file takes the cost the file records, whatever this says. [PasswordHashing](./password-hashing.md) has the limits.

## AsyncOpenOptions

```ts
interface AsyncOpenOptions<S = Schema> extends Omit<OpenOptions<S>, 'migrations'> {
  migrations?: AsyncMigration<S>[];
}
```

The options of `Database.openAsync`. They are those of `OpenOptions`, except that `migrations` holds [AsyncMigration](./migration.md#asyncmigration) objects, whose `run` may be asynchronous and receives the asynchronous API. `openAsync` copies the key or password when it is called, before the work moves to the thread pool, so a buffer can be wiped as soon as the call returns rather than when its promise settles.
