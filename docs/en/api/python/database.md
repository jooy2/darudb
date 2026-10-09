---
title: Database
order: 1
---

# Database

`Database` is an open DaruDB file in Python, through which a program runs transactions, prepares queries and uses the file's tools.

```python
class Database: ...
```

There is no constructor: `Database.open` and `Database.open_async` return one, and calling `Database()` raises `TypeError`. A database opened without a [schema](./schema.md) has no collections, and one opened with a schema reaches each collection through its class, as `txn.collection(User)`.

A `Database` works until `close` or `close_async` has closed it. After that `path`, `schema`, `schema_version` and `is_open` can still be read, and every member that uses the file raises a [DaruError](../../types/python/error.md) with the code `CLOSED`. Opening a file that the process already has open gives another handle to the same database, with the same page cache, and each handle keeps the schema it was opened with.

The native module releases the GIL for everything the engine does, so other threads run while one waits for the disk or for another writer. A database can be used from several threads at once, and a transaction from any thread, one call at a time. Every method that uses the file has a twin for `asyncio` whose name ends in `_async`, which does the engine's work on a thread pool the package keeps, of at most `min(32, os.cpu_count() + 4)` threads, so that an event loop never waits for the disk or for another process's writer. [Asynchronous API](../../guide/async.md) explains how the two kinds of call share a file.

## Class methods

### open

```python
@classmethod
def open(
    cls,
    path: str | os.PathLike[str],
    *,
    schema: Schema | None = None,
    migrations: Sequence[Migration] = (),
    create: bool = True,
    page_size: int | None = None,
    busy_timeout: float | None = None,
    cache_size: int | None = None,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> Database: ...
```

Opens the database at `path`, creating it if nothing exists there, and stores, checks or migrates its schema.

| Option | Description |
| --- | --- |
| `schema` | The collections the file holds, as a [Schema](./schema.md). The first open stores it; later opens check it, and migrate a file that holds an older version |
| `migrations` | The [Migration](./migration.md) steps, one for each version that needs more than the engine does by itself |
| `create` | Whether a missing file is created. `False` fails with `NOT_FOUND` instead |
| `page_size` | The page size of a new file: a power of two from 4096 to 65536, 4096 by default. An existing file keeps its own |
| `busy_timeout` | How long, in seconds, opening and a write wait for another writer before failing with `BUSY`: 5 by default |
| `cache_size` | The memory the page cache may take, in bytes: 32 MiB by default |
| `key` | A key of 32 bytes, which encrypts a new file and opens an encrypted one |
| `password` | A password, which does what `key` does through a key derived with Argon2id. A key and a password together fail with `INVALID_ARGUMENT` |
| `password_hashing` | What deriving a key from a password costs, for a new file and for `set_password`. See [PasswordHashing](../../types/python/password-hashing.md) |

Migration functions run inside this call, one version step after another, and receive a [Migrating](./migrating.md). They have to be plain functions here: a coroutine function fails with `INVALID_ARGUMENT`, whose message names `open_async`. If a function raises, the file keeps its old schema and data, and `open` raises the same error.

- `NOT_FOUND`: nothing exists at `path` and `create` is `False`.
- `NOT_A_DATABASE`: the file is not a DaruDB database.
- `KEY_REQUIRED` and `WRONG_KEY`: the file is encrypted, and neither `key` nor `password` was given, or the wrong one.
- `SCHEMA_MISMATCH`: the file holds another schema at the same version. `SCHEMA_TOO_NEW`: the file holds a newer version.
- `BUSY`: another process kept the file busy for longer than `busy_timeout`, or a salvage holds it.
- `INVALID_ARGUMENT`: an option cannot be used, such as a page size that is not a power of two from 4096 to 65536, a negative or NaN `busy_timeout`, a key that is not 32 bytes, both a key and a password, an empty password, a key or a password for a plain file that exists, migrations without a schema, two migrations to one version, or a schema the engine cannot store.

```python
import darudb


@darudb.collection("users")
class User:
    id: int | None = None
    name: str


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))
```

A `bytearray` key or password can be wiped as soon as `open` returns. A `bytes` or a `str` cannot be wiped, and stays in memory until Python reclaims it.

### open_async

```python
@classmethod
async def open_async(
    cls,
    path: str | os.PathLike[str],
    *,
    schema: Schema | None = None,
    migrations: Sequence[Migration] = (),
    create: bool = True,
    page_size: int | None = None,
    busy_timeout: float | None = None,
    cache_size: int | None = None,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> Database: ...
```

`open` on the package's thread pool, so that the event loop does not wait for the file, a recovery or a migration's commit. A migration's `run` may be a coroutine function here, and receives an [AsyncMigrating](./migrating.md#asyncmigrating); a plain function works too. A step ends once the function has returned, its coroutine included, and every operation it started has finished. It fails as `open` does. The key and the password are read when the work runs on the pool, so a `bytearray` can be wiped once the `await` has returned, not before.

```python
db = await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User]))
```

## Static methods

### salvage

```python
@staticmethod
def salvage(
    source: str | os.PathLike[str],
    target: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    busy_timeout: float | None = None,
) -> SalvageReport: ...
```

Rescues what it can of the damaged database at `source` into a new database at `target`, and reports what it rescued and what it could not. It reads the file page by page, so it works on a file that does not open. It starts from the newest commit the file records, takes what that commit cannot read from older versions of the same pages, and builds every index again, so the new file passes the integrity check. An encrypted file needs its `key` or `password`, and the new file opens with the same one. `busy_timeout` is how long it waits for other processes to close the file. [SalvageReport](../../types/python/salvage-report.md) describes the result, and [Tools](../../guide/tools.md) explains when to use it.

- `BUSY`: the file is open in this process, which fails at once, or in another process for longer than `busy_timeout`. Opening the file while salvage runs fails with `BUSY` too.
- `INVALID_ARGUMENT`: something already exists at `target`, or both a key and a password were given. Salvage never replaces a file.
- `NOT_FOUND`: nothing exists at `source`.
- `KEY_REQUIRED`: the file is encrypted and neither a key nor a password was given.

### salvage_async

```python
@staticmethod
async def salvage_async(
    source: str | os.PathLike[str],
    target: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    busy_timeout: float | None = None,
) -> SalvageReport: ...
```

`salvage` on the package's thread pool.

## Properties

### path

```python
path: str
```

The path the database was opened at, as `os.fspath` gives it for the path passed to `open`. It can still be read after `close`.

### schema

```python
schema: Schema | None
```

The [Schema](./schema.md) the database was opened with, or `None`. It can still be read after `close`.

### is_open

```python
@property
def is_open(self) -> bool: ...
```

Whether the database is open. It turns `False` once `close` or `close_async` has closed it.

### page_size

```python
@property
def page_size(self) -> int: ...
```

The size of every page in the file, in bytes. A file keeps the page size it was created with.

### format_version

```python
@property
def format_version(self) -> int: ...
```

The file format version recorded in the file. A file this build opens has the version of [FORMAT_VERSION](../../types/python/constants.md).

### is_encrypted

```python
@property
def is_encrypted(self) -> bool: ...
```

Whether the file is encrypted.

### schema_version

```python
@property
def schema_version(self) -> int | None: ...
```

The version of the schema the database was opened with, which is the version the file held once it opened, or `None` when it was opened without a schema. It comes from `schema`, so it can still be read after `close`.

## Methods

### prepare

```python
def prepare(self, collection: type[T] | str, query: Query | Condition | str) -> Prepared[T]: ...
```

Prepares a query once on `collection`, given as its class or its name, so that each run gives only the values of its parameters. The query is text in the query language with `$0`, `$1` and on, or a [Query](./query.md) or a [condition](./conditions.md) built with [param](./param.md) in place of values. The [Prepared](../../types/python/prepared.md) query holds no database or transaction, so it runs in any transaction on that collection, synchronous or asynchronous.

It fails with `INVALID_ARGUMENT` for a collection the schema does not have, and when the database was opened without a schema, with `INVALID_QUERY` for text that does not parse, and with `CLOSED` after the database is closed. A field the collection does not have is found when the query runs.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))

with db.read() as txn:
    alice = txn.collection(User).find_one(by_email, "alice@example.com")
```

### read

```python
def read(self) -> _ReadScope: ...
```

A read transaction, as a context manager: `with db.read() as txn` gives a [ReadTransaction](./read-transaction.md), which sees one commit until the block ends. Beginning it waits for no writer.

### write

```python
def write(self, *, durability: Durability = "sync") -> _WriteScope: ...
```

The write transaction, as a context manager: `with db.write() as txn` gives a [WriteTransaction](./write-transaction.md), commits it when the block ends and aborts it when the block raises. By default the commit returns once it is durable; `durability="deferred"` returns without waiting for the disk, as [Durability](../../types/python/durability.md) describes. Any other `durability` fails with `INVALID_ARGUMENT` when `write` is called.

- `BUSY`: another writer, in another thread or another process, held the file for longer than `busy_timeout`.
- `INVALID_ARGUMENT`: a write block of this thread already holds the file, through this handle or another, or an asynchronous write of the event loop running on this thread holds it. Write transactions do not nest, and waiting here would wait for a write that cannot end until this one does.
- `SYNC_FAILED`: the disk failed the commit's barrier, raised when the block ends. The database has to be closed and opened again.

### read_async

```python
def read_async(self) -> AsyncReadScope: ...
```

A read transaction for `async with`: `async with db.read_async() as txn` gives an [AsyncReadTransaction](./read-transaction.md#asyncreadtransaction), whose operations are awaited and run on the thread pool. The transaction begins on the calling thread, since beginning a read waits for no writer.

### write_async

```python
def write_async(self, *, durability: Durability = "sync") -> AsyncWriteScope: ...
```

The write transaction for `async with`: `async with db.write_async() as txn` gives an [AsyncWriteTransaction](./write-transaction.md#asyncwritetransaction). It commits when the block ends, once every operation it started has finished, and aborts when the block raises. This process's asynchronous writes on one file, from one event loop, take turns: each waits on the event loop for the ones before it to end, and only then takes a thread of the pool, so a write that waits holds no thread.

It fails with `INVALID_ARGUMENT` when a synchronous write block of this thread holds the file, and when it is awaited inside the block of an asynchronous write on the same file, or in a task made inside that block: it would wait for the write it is part of. `sync_async`, `close_async`, `compact_async`, `set_key_async` and `set_password_async`, which take their turn with the writes, are refused there too.

### check

```python
def check(self) -> CheckReport: ...
```

Checks the published commit completely: every page against its check, the order of every key, every count, that every page is used, free or retained exactly once, and every object against its indexes. It returns every problem it finds in a [CheckReport](../../types/python/check-report.md) rather than raising, and it reads while other handles and processes write.

### check_async

```python
async def check_async(self) -> CheckReport: ...
```

`check` on the thread pool. It never waits for a writer.

### backup

```python
def backup(
    self,
    path: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> BackupReport: ...
```

Writes a copy of the published commit to a new file at `path`, while other handles and processes may write. The copy holds no free space, has the file's page size, and opens with the same key or password. It never replaces a file: a path that is taken fails with `INVALID_ARGUMENT`. [BackupReport](../../types/python/backup-report.md) describes the result.

A `key` of 32 bytes or a `password` encrypts the copy under a new random data key, at the cost [`password_hashing`](../../types/python/password-hashing.md) sets for a password, and the copy opens only with it. Changing a file's key or password wraps its data key again and leaves the data key as it was, so a backup under a new key is the way to leave behind a data key that may have been exposed: back up, then put the copy in the old file's place. A plain database's copy is encrypted the same way. A key that is not 32 bytes, or a key and a password together, fail with `INVALID_ARGUMENT` before anything is written. [Tools](../../guide/tools.md) explains how a backup is made.

### backup_async

```python
async def backup_async(
    self,
    path: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> BackupReport: ...
```

`backup` on the thread pool. The key and the password are read when the work runs, as in `open_async`.

### compact

```python
def compact(self) -> CompactReport: ...
```

Makes the file smaller in place: the trees whose pages inserts left part empty are written again, full, and pages at its end move into free pages nearer its start, and the end goes back to the file system. It works in write transactions of its own, so it waits for the writer as a write does. It is refused with `INVALID_ARGUMENT`, at once, inside a write block of this thread on the same file, through any handle to it, and on the thread of an event loop whose asynchronous write holds the file, since either way it would wait for a write that cannot end until it returns. A page that a read transaction can still reach does not move. [CompactReport](../../types/python/compact-report.md) describes the result.

### compact_async

```python
async def compact_async(self) -> CompactReport: ...
```

`compact` on the thread pool, after this event loop's earlier asynchronous writes on the file.

### set_key

```python
def set_key(self, key: bytes | bytearray | memoryview) -> None: ...
```

Changes the key of an encrypted database to `key`, which is 32 bytes. No page is encrypted again, and once it returns, the old key or password no longer opens the file. It commits, so it waits for the writer and is refused where `compact` is. A plain database, or a key that is not 32 bytes, fails with `INVALID_ARGUMENT`. [Encryption](../../guide/encryption.md) covers keys and passwords.

### set_key_async

```python
async def set_key_async(self, key: bytes | bytearray | memoryview) -> None: ...
```

`set_key` on the thread pool, after this event loop's earlier asynchronous writes on the file. The key is read when the work runs, so a `bytearray` can be wiped once the `await` has returned, not before.

### set_password

```python
def set_password(self, password: str | bytes | bytearray) -> None: ...
```

Changes the key of an encrypted database to one derived from `password` with Argon2id, at the cost that `password_hashing` set when the process opened the file, or the default cost. It is refused and fails as `set_key` is.

### set_password_async

```python
async def set_password_async(self, password: str | bytes | bytearray) -> None: ...
```

`set_password` on the thread pool, after this event loop's earlier asynchronous writes on the file. The password is read when the work runs, as the key of `set_key_async` is.

### sync

```python
def sync(self) -> None: ...
```

Makes every commit durable, deferred ones included, whichever handle or process made them. It may wait for the writer, so it is refused where `compact` is.

### sync_async

```python
async def sync_async(self) -> None: ...
```

`sync` on the thread pool, after this event loop's earlier asynchronous writes on the file.

### close

```python
def close(self) -> None: ...
```

Makes deferred commits durable and closes this handle. Closing a database that is already closed does nothing. It may wait for the writer, so it is refused where `compact` is, and a refused `close` leaves the database open.

### close_async

```python
async def close_async(self) -> None: ...
```

`close` on the thread pool, after this event loop's earlier asynchronous writes on the file.

## Context manager

```python
def __enter__(self) -> Self: ...
def __exit__(
    self,
    kind: type[BaseException] | None,
    error: BaseException | None,
    trace: TracebackType | None,
) -> None: ...
async def __aenter__(self) -> Self: ...
async def __aexit__(
    self,
    kind: type[BaseException] | None,
    error: BaseException | None,
    trace: TracebackType | None,
) -> None: ...
```

A database closes itself when its block ends, whether or not the block raised, and an error the block raised goes on. `with` closes it with `close`, and `async with` with `close_async`.

```python
with darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User])) as db:
    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))


async def main() -> None:
    async with await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User])) as db:
        async with db.read_async() as txn:
            print(await txn.collection(User).count())
```
