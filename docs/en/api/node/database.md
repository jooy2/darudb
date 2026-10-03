---
title: Database
order: 1
---

# Database

`Database` is an open DaruDB file in Node.js, through which a program runs transactions, prepares queries and uses the file's tools.

```ts
interface Database<S extends Schema<any> = Schema>
```

There is no constructor: `Database.open` and `Database.openAsync` return one, and calling `new Database()` throws. The static methods below belong to the `Database` value, whose type is `DatabaseOpener`. `S` is the [schema](./schema.md) the database was opened with. It carries the names of the collections and the types of their objects into every transaction, and nothing at run time. A database opened without a schema has no collections.

A `Database` works until `close` or `closeAsync` is called. After that only `path` and `isOpen` can be read, and every other member throws `CLOSED`. Opening a file that the process already has open gives another handle to the same database, with the same page cache, and each handle keeps the schema it was opened with.

Every method that uses the file has a twin whose name ends in `Async`. The twin does the engine's work on the libuv thread pool and returns a promise, so the event loop never waits for the disk or for another process's writer. [Asynchronous API](../../guide/async.md) explains how the two kinds of call share a file.

## Static methods

### open

```ts
open<S extends Schema<any>>(path: string, options: OpenOptions<S> & { schema: S }): Database<S>;
open(path: string, options?: OpenOptions<never>): Database;
```

Opens the database at `path`, creating it if nothing exists there, and stores, checks or migrates its schema. [OpenOptions](../../types/node/open-options.md) lists the options. Migration functions run synchronously, inside this call, and receive a [Migrating](./migrating.md). If one throws, the file keeps its old schema and data, and `open` throws the same error.

- `NOT_FOUND`: nothing exists at `path` and `create` is `false`.
- `NOT_A_DATABASE`: the file is not a DaruDB database.
- `KEY_REQUIRED` and `WRONG_KEY`: the file is encrypted, and the options have no key or password, or the wrong one.
- `SCHEMA_MISMATCH`: the file holds another schema at the same version. `SCHEMA_TOO_NEW`: the file holds a newer version.
- `BUSY`: another process kept the file busy for longer than `busyTimeout`, or a salvage holds it.
- `INVALID_ARGUMENT`: an option cannot be used, such as a page size that is not a power of two from 4096 to 65536, a key that is not 32 bytes, both a key and a password, or a schema the engine cannot store.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, { users: collection({ name: t.string() }) });
const db = Database.open('app.darudb', { schema: app });
```

### openAsync

```ts
openAsync<S extends Schema<any>>(
  path: string,
  options: AsyncOpenOptions<S> & { schema: S }
): Promise<Database<S>>;
openAsync(path: string, options?: AsyncOpenOptions<never>): Promise<Database>;
```

`open` on the thread pool. Its migration functions may be asynchronous and receive an [AsyncMigrating](./migrating.md#asyncmigrating); a step ends once the function's promise has settled and every operation it called has too. It fails as `open` does, by rejecting.

### salvage

```ts
salvage(from: string, into: string, options?: SalvageOptions): SalvageReport;
```

Rescues what it can of the damaged database at `from` into a new database at `into`, and reports what it rescued and what it could not. It reads the file page by page, so it works on a file that does not open. It starts from the newest commit the file records, takes what that commit cannot read from older versions of the same pages, and builds every index again, so the new file passes the integrity check. An encrypted file needs its key or password in [SalvageOptions](../../types/node/salvage-options.md), and the new file opens with the same one. [SalvageReport](../../types/node/salvage-report.md) describes the result, and [Tools](../../guide/tools.md) explains when to use it.

- `BUSY`: the file is open in this process or another. Opening the file while salvage runs fails with `BUSY` too.
- `INVALID_ARGUMENT`: something already exists at `into`, or a path is empty. Salvage never replaces a file.
- `NOT_FOUND`: nothing exists at `from`.
- `KEY_REQUIRED`: the file is encrypted and the options have no key or password.

### salvageAsync

```ts
salvageAsync(from: string, into: string, options?: SalvageOptions): Promise<SalvageReport>;
```

`salvage` on the thread pool.

## Properties

### path

```ts
readonly path: string;
```

The path the database was opened at, as it was given. It can still be read after `close`.

### isOpen

```ts
readonly isOpen: boolean;
```

Whether the database is open. It turns `false` as soon as `close` or `closeAsync` is called.

### pageSize

```ts
readonly pageSize: number;
```

The size of every page in the file, in bytes. A file keeps the page size it was created with.

### formatVersion

```ts
readonly formatVersion: number;
```

The file format version recorded in the file. A file this build opens has the version of [FORMAT_VERSION](../../types/node/constants.md).

### isEncrypted

```ts
readonly isEncrypted: boolean;
```

Whether the file is encrypted.

### schemaVersion

```ts
readonly schemaVersion: number | null;
```

The version of the schema the file held when this handle opened it, or `null` when it was opened without a schema.

## Methods

### prepare

```ts
prepare<N extends NameOf<S>>(
  collection: N,
  query: QueryInput<ObjectOf<FieldsOf<S, N>>> | string
): Prepared<ObjectOf<FieldsOf<S, N>>>;
```

Prepares a query on collection `collection` once, so that each run gives only the values of its parameters. The query is text in the query language with `$0`, `$1` and on, or a query built with [param](./param.md) in place of values. `N` is one of the schema's collection names, and `FieldsOf<S, N>` its fields. The [Prepared](../../types/node/prepared.md) query holds no database or transaction, so it runs in any transaction on that collection, synchronous or asynchronous.

It fails with `INVALID_ARGUMENT` for a collection the schema does not have, and with `INVALID_QUERY` for text that does not parse.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const alice = db.read((txn) => txn.collection('users').findOne(byEmail, ['alice@example.com']));
```

### read

```ts
read<R>(fn: (txn: ReadTransaction<S>) => R): R;
```

Runs `fn` in a [read transaction](./read-transaction.md) and returns what `fn` returns. The transaction sees one commit for as long as `fn` runs, and beginning it waits for no writer. `fn` has to be synchronous: one that returns a promise is refused with `INVALID_ARGUMENT`.

### readAsync

```ts
readAsync<R>(fn: (txn: AsyncReadTransaction<S>) => R): Promise<Awaited<R>>;
```

`read` with the asynchronous API: `fn` may be asynchronous, its collections' operations run on the thread pool, and the transaction sees one commit until `fn` settles. The transaction begins on the calling thread, since beginning a read waits for no writer and costs less than a trip to the pool.

### write

```ts
write<R>(fn: (txn: WriteTransaction<S>) => R, options?: WriteOptions): R;
```

Runs `fn` in a [write transaction](./write-transaction.md), commits it when `fn` returns, aborts it when `fn` throws, and returns what `fn` returns. By default the commit returns once it is durable; `{ durability: 'deferred' }` returns without waiting for the disk, as [WriteOptions](../../types/node/write-options.md) describes.

- `BUSY`: another process's writer held the file for longer than `busyTimeout`.
- `INVALID_ARGUMENT`: `fn` returned a promise, and the transaction was aborted.
- `INVALID_ARGUMENT` also when the call comes from inside a write transaction's function on the same file, or while an asynchronous write of this process holds the file. Write transactions do not nest, and waiting here would block the thread that the other write needs to finish.
- `SYNC_FAILED`: the disk failed the commit's barrier. The database has to be closed and opened again.

### writeAsync

```ts
writeAsync<R>(
  fn: (txn: AsyncWriteTransaction<S>) => R,
  options?: WriteOptions
): Promise<Awaited<R>>;
```

`write` with the asynchronous API: `fn` may be asynchronous, the transaction commits once `fn` resolves and every operation it called has settled, and it aborts when `fn` rejects. This process's asynchronous writes on one file, through any number of `Database` objects, wait their turn in JavaScript and reach the thread pool one at a time, so a write that waits holds no thread. Called from inside the function of an asynchronous write on the same file, it rejects with `INVALID_ARGUMENT` rather than wait for itself.

### check

```ts
check(): CheckReport;
```

Checks the published commit completely: every page against its check, the order of every key, every count, that every page is used, free or retained exactly once, and every object against its indexes. It returns every problem it finds in a [CheckReport](../../types/node/check-report.md) rather than throwing, and it reads while other handles and processes write.

### checkAsync

```ts
checkAsync(): Promise<CheckReport>;
```

`check` on the thread pool. It never waits for a writer.

### backup

```ts
backup(path: string): BackupReport;
```

Writes a copy of the published commit to a new file at `path`, while other handles and processes may write. The copy holds no free space, has the file's page size, and opens with the same key or password. It never replaces a file: a path that is taken, or an empty one, fails with `INVALID_ARGUMENT`. [BackupReport](../../types/node/backup-report.md) describes the result.

### backupAsync

```ts
backupAsync(path: string): Promise<BackupReport>;
```

`backup` on the thread pool.

### compact

```ts
compact(): CompactReport;
```

Makes the file smaller in place: pages at its end move into free pages nearer its start, and the end goes back to the file system. It works in write transactions of its own, so it waits for the writer lock as a write does, and it is refused with `INVALID_ARGUMENT` inside a write transaction's function on the same file and while an asynchronous write of this process holds the file, as `write` is. A page that a read transaction can still reach does not move. [CompactReport](../../types/node/compact-report.md) describes the result.

### compactAsync

```ts
compactAsync(): Promise<CompactReport>;
```

`compact` on the thread pool, after this process's earlier writes on the file.

### setKey

```ts
setKey(key: Uint8Array): void;
```

Changes the key of an encrypted database to `key`, which is 32 bytes. No page is encrypted again, and once it returns, the old key or password no longer opens the file. It commits, so it is refused with `INVALID_ARGUMENT` inside a write transaction's function on the same file and while an asynchronous write of this process holds the file, as `write` is. A plain database, or a key that is not 32 bytes, fails with `INVALID_ARGUMENT`. The package copies the key when the call is made, so the caller may wipe its own buffer afterwards. [Encryption](../../guide/encryption.md) covers keys and passwords.

### setKeyAsync

```ts
setKeyAsync(key: Uint8Array): Promise<void>;
```

`setKey` on the thread pool, after this process's earlier writes on the file. The key is copied when the call is made, before the work waits its turn.

### setPassword

```ts
setPassword(password: string | Uint8Array): void;
```

Changes the key of an encrypted database to one derived from `password` with Argon2id, at the cost that `passwordHashing` set when the database was opened, or the default cost. It is refused and fails as `setKey` is. A `Uint8Array` can be wiped once the call returns; a string stays in memory until the garbage collector reclaims it.

### setPasswordAsync

```ts
setPasswordAsync(password: string | Uint8Array): Promise<void>;
```

`setPassword` on the thread pool, after this process's earlier writes on the file.

### sync

```ts
sync(): void;
```

Makes every commit durable, deferred ones included. It may wait for the writer, so it is refused with `INVALID_ARGUMENT` inside a write transaction's function on the same file and while an asynchronous write of this process holds the file, as `write` is.

### syncAsync

```ts
syncAsync(): Promise<void>;
```

`sync` on the thread pool, after this process's earlier writes on the file.

### close

```ts
close(): void;
```

Makes deferred commits durable and closes the database. Closing a database that is already closed does nothing. It may wait for the writer, so it is refused with `INVALID_ARGUMENT` inside a write transaction's function on the same file and while an asynchronous write of this process holds the file, as `write` is. A refused `close` leaves the database open.

### closeAsync

```ts
closeAsync(): Promise<void>;
```

`close` on the thread pool, after this process's earlier writes on the file. The database refuses new work as soon as the call is made, and `isOpen` turns `false` at once. Called from inside the function of an asynchronous write on the same file, it rejects with `INVALID_ARGUMENT` and leaves the database open.
