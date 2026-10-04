---
title: Database
order: 1
---

# Database

`Database` is an open DaruDB file in Dart, through which a program runs transactions, prepares queries and uses the file's tools.

```dart
final class Database implements Finalizable
```

There is no constructor: `Database.open` and `Database.openAsync` return one. A database opened without a [schema](./schema.md) has no collections, and one opened with a schema reaches each collection through the constant `darudb_generator` wrote for its class, such as `userSchema`.

A `Database` works until `close` or `closeAsync` has closed it. After that only `path` and `isOpen` can be read, and every other member throws a [DaruException](../../types/dart/error.md) with the code `CLOSED`. Opening a file that the process already has open, from this isolate or another, gives another handle to the same database, with the same page cache, and each handle keeps the schema it was opened with. A handle that is never closed is closed when the garbage collector reclaims it, but a program should not count on when that happens.

Every call runs on the isolate that makes it and holds it until the engine returns, so a write that waits for another writer, or a sync commit that waits for the disk, holds a Flutter app's UI isolate too. Every method that uses the file has a twin whose name ends in `Async`, which does the engine's work on a thread of the package's native library and returns a `Future`. [Asynchronous API](../../guide/async.md) explains how the two kinds of call share a file.

## Static methods

### open

```dart
static Database open(
  String path, {
  Schema? schema,
  List<Migration> migrations = const [],
  bool create = true,
  int? pageSize,
  Duration? busyTimeout,
  int? cacheSize,
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

Opens the database at `path`, creating it if nothing exists there, and stores, checks or migrates its schema.

| Option | Description |
| --- | --- |
| `schema` | The collections the file holds. The first open stores it; later opens check it, and migrate a file that holds an older version |
| `migrations` | The [Migration](./migration.md) steps, one for each version that needs more than the engine does by itself |
| `create` | Whether a missing file is created. `false` fails with `NOT_FOUND` instead |
| `pageSize` | The page size of a new file: a power of two from 4096 to 65536, 4096 by default. An existing file keeps its own |
| `busyTimeout` | How long a write waits for another process's writer before failing with `BUSY`: five seconds by default |
| `cacheSize` | The memory the page cache may take, in bytes: 32 MiB by default |
| `key` | A key of 32 bytes, which encrypts a new file and opens an encrypted one |
| `password` | A password, which does what `key` does through a key derived with Argon2id |
| `passwordHashing` | What deriving a key from a password costs, for a new file and for `setPassword`. See [PasswordHashing](../../types/dart/password-hashing.md) |

Migration functions run inside this call, one version step after another, and receive a [MigrationContext](./migration-context.md). They have to be synchronous here: one that returns a `Future` fails with `INVALID_ARGUMENT`. If one throws, the file keeps its old schema and data, and `open` throws the same error.

- `NOT_FOUND`: nothing exists at `path` and `create` is `false`.
- `NOT_A_DATABASE`: the file is not a DaruDB database.
- `KEY_REQUIRED` and `WRONG_KEY`: the file is encrypted, and neither `key` nor `password` was given, or the wrong one.
- `SCHEMA_MISMATCH`: the file holds another schema at the same version. `SCHEMA_TOO_NEW`: the file holds a newer version.
- `BUSY`: another process kept the file busy for longer than `busyTimeout`, or a salvage holds it.
- `INVALID_ARGUMENT`: an option cannot be used, such as a page size that is not a power of two from 4096 to 65536, a key that is not 32 bytes, both a key and a password, or a schema the engine cannot store.

```dart
final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));
```

The package copies the key and the password into native memory for the call, and fills its copies with zeros once the engine has its own. A `Uint8List` key can be wiped with `fillRange` as soon as `open` returns; a `String` cannot be wiped.

### openAsync

```dart
static Future<Database> openAsync(
  String path, {
  Schema? schema,
  List<Migration> migrations = const [],
  bool create = true,
  int? pageSize,
  Duration? busyTimeout,
  int? cacheSize,
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

`open` on a thread of the native library, so that the isolate does not wait for the file, a recovery or a migration's commit. Its migration functions may be asynchronous, and a step ends once the function's `Future` completes. It fails as `open` does, with a `Future` that completes with the error.

### salvage

```dart
static SalvageReport salvage(
  String from,
  String into, {
  Duration? busyTimeout,
  Uint8List? key,
  String? password,
});
```

Rescues what it can of the damaged database at `from` into a new database at `into`, and reports what it rescued and what it could not. It reads the file page by page, so it works on a file that does not open. It starts from the newest commit the file records, takes what that commit cannot read from older versions of the same pages, and builds every index again, so the new file passes the integrity check. An encrypted file needs its `key` or `password`, and the new file opens with the same one. `busyTimeout` is how long it waits for other processes to close the file. [SalvageReport](../../types/dart/salvage-report.md) describes the result, and [Tools](../../guide/tools.md) explains when to use it.

- `BUSY`: the file is open in this process or another. Opening the file while salvage runs fails with `BUSY` too.
- `INVALID_ARGUMENT`: something already exists at `into`, or a path is empty. Salvage never replaces a file.
- `NOT_FOUND`: nothing exists at `from`.
- `KEY_REQUIRED`: the file is encrypted and neither a key nor a password was given.

### salvageAsync

```dart
static Future<SalvageReport> salvageAsync(
  String from,
  String into, {
  Duration? busyTimeout,
  Uint8List? key,
  String? password,
});
```

`salvage` on a thread of the native library.

## Properties

### path

```dart
final String path;
```

The path the database was opened at, as it was given. It can still be read after `close`.

### isOpen

```dart
bool get isOpen;
```

Whether the database is open. It turns `false` once `close` or `closeAsync` has closed it.

### pageSize

```dart
int get pageSize;
```

The size of every page in the file, in bytes. A file keeps the page size it was created with.

### formatVersion

```dart
int get formatVersion;
```

The file format version recorded in the file. A file this build opens has the version of the top-level [formatVersion](../../types/dart/constants.md).

### isEncrypted

```dart
bool get isEncrypted;
```

Whether the file is encrypted.

### schemaVersion

```dart
int? get schemaVersion;
```

The version of the schema the file held when this handle opened it, or `null` when it was opened without a schema.

## Methods

### prepare

```dart
Prepared<T> prepare<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> collection,
  String text,
);
```

Parses `text`, a query in the query language with `$0`, `$1` and on in place of the values that change, once for `collection`. The [Prepared](../../types/dart/prepared.md) query holds no transaction, so it runs in any transaction on that collection, synchronous or asynchronous, through `findPrepared`, `findOnePrepared` and `countPrepared`. It fails with `INVALID_QUERY` for text that does not parse.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');
final alice = db.read(
  (txn) => txn.collection(userSchema).findOnePrepared(byEmail, ['alice@example.com']),
);
```

### read

```dart
R read<R>(R Function(ReadTransaction txn) fn);
```

Runs `fn` in a [read transaction](./read-transaction.md) and returns what `fn` returns. The transaction sees one commit for as long as `fn` runs, and beginning it waits for no writer. `fn` has to be synchronous: one that returns a `Future` fails with `INVALID_ARGUMENT`.

### readAsync

```dart
Future<R> readAsync<R>(FutureOr<R> Function(AsyncReadTransaction txn) fn);
```

`read` with the `Future` API: `fn` may be asynchronous, its collections' calls run on threads of the native library, and the transaction sees one commit until `fn` completes. The transaction begins on the calling isolate, since beginning a read waits for no writer.

### write

```dart
R write<R>(
  R Function(WriteTransaction txn) fn, {
  Durability durability = Durability.sync,
});
```

Runs `fn` in a [write transaction](./write-transaction.md), commits it when `fn` returns, aborts it when `fn` throws, and returns what `fn` returns. By default the commit returns once it is durable; `Durability.deferred` returns without waiting for the disk, as [Durability](../../types/dart/durability.md) describes.

- `BUSY`: another process's writer held the file for longer than `busyTimeout`.
- `INVALID_ARGUMENT`: `fn` returned a `Future`, and the transaction was aborted.
- `INVALID_ARGUMENT` also when the call comes from inside a write transaction's function on the same file, or while an asynchronous write of this isolate holds the file. Write transactions do not nest, and waiting here would hold the isolate that the other write needs to finish.
- `SYNC_FAILED`: the disk failed the commit's barrier. The database has to be closed and opened again.

### writeAsync

```dart
Future<R> writeAsync<R>(
  FutureOr<R> Function(AsyncWriteTransaction txn) fn, {
  Durability durability = Durability.sync,
});
```

`write` with the `Future` API: `fn` may be asynchronous, the transaction commits once `fn` completes and every call it made has finished, and it aborts when `fn` fails. This isolate's asynchronous writes on one file, through any number of `Database` objects, wait their turn in Dart and reach the native library one at a time. Called from inside the function of an asynchronous write on the same file, it fails with `INVALID_ARGUMENT` rather than wait for itself.

### check

```dart
CheckReport check();
```

Checks the published commit completely: every page against its check, the order of every key, every count, that every page is used, free or retained exactly once, and every object against its indexes. It returns every problem it finds in a [CheckReport](../../types/dart/check-report.md) rather than throwing, and it reads while other handles and processes write.

### checkAsync

```dart
Future<CheckReport> checkAsync();
```

`check` on a thread of the native library. It never waits for a writer.

### backup

```dart
BackupReport backup(
  String path, {
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

Writes a copy of the published commit to a new file at `path`, while other handles and processes may write. The copy holds no free space, has the file's page size, and opens with the same key or password. It never replaces a file: a path that is taken, or an empty one, fails with `INVALID_ARGUMENT`. [BackupReport](../../types/dart/backup-report.md) describes the result.

A `key` of 32 bytes or a `password` encrypts the copy under a new random data key, which it wraps, at the cost [`passwordHashing`](../../types/dart/password-hashing.md) sets for a password, and the copy opens only with it. Changing a file's key or password wraps its data key again and leaves it as it was, so a backup under a new key is the way to leave behind a data key that may have been exposed: back up, then put the copy in the old file's place. A plain database's copy is encrypted the same way. A key that is not 32 bytes, an empty password, or both together fail with `INVALID_ARGUMENT` before anything is written, and the package copies them when the call is made.

### backupAsync

```dart
Future<BackupReport> backupAsync(
  String path, {
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

`backup` on a thread of the native library.

### compact

```dart
CompactReport compact();
```

Makes the file smaller in place: pages at its end move into free pages nearer its start, and the end goes back to the file system. It works in write transactions of its own, so it waits for the writer as a write does, and it is refused with `INVALID_ARGUMENT` where `write` is. A page that a read transaction can still reach does not move. [CompactReport](../../types/dart/compact-report.md) describes the result.

### compactAsync

```dart
Future<CompactReport> compactAsync();
```

`compact` on a thread of the native library, after this isolate's earlier asynchronous writes on the file.

### setKey

```dart
void setKey(Uint8List key);
```

Changes the key of an encrypted database to `key`, which is 32 bytes. No page is encrypted again, and once it returns, the old key or password no longer opens the file. It commits, so it is refused with `INVALID_ARGUMENT` where `write` is. A plain database, or a key that is not 32 bytes, fails with `INVALID_ARGUMENT`. [Encryption](../../guide/encryption.md) covers keys and passwords.

### setKeyAsync

```dart
Future<void> setKeyAsync(Uint8List key);
```

`setKey` on a thread of the native library, after this isolate's earlier asynchronous writes on the file. The key is copied when the call is made, so the caller may wipe its own list at once.

### setPassword

```dart
void setPassword(String password);
```

Changes the key of an encrypted database to one derived from `password` with Argon2id, at the cost that `passwordHashing` set when the file was opened, or the default cost. It is refused and fails as `setKey` is.

### setPasswordAsync

```dart
Future<void> setPasswordAsync(String password);
```

`setPassword` on a thread of the native library, after this isolate's earlier asynchronous writes on the file.

### sync

```dart
void sync();
```

Makes every commit durable, deferred ones included, whichever handle or process made them. It may wait for the writer, so it is refused with `INVALID_ARGUMENT` where `write` is.

### syncAsync

```dart
Future<void> syncAsync();
```

`sync` on a thread of the native library, after this isolate's earlier asynchronous writes on the file.

### close

```dart
void close();
```

Makes deferred commits durable and closes the database. Closing a database that is already closed does nothing. It may wait for the writer, so it is refused with `INVALID_ARGUMENT` where `write` is, and a refused `close` leaves the database open.

### closeAsync

```dart
Future<void> closeAsync();
```

`close` on a thread of the native library, after this isolate's earlier asynchronous writes on the file. Called from inside the function of an asynchronous write on the same file, it fails with `INVALID_ARGUMENT` and leaves the database open.
