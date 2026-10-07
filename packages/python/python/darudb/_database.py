"""The database, its transactions and its collections, and the tools.

A transaction is a context manager: ``with db.read() as txn`` sees one commit
throughout, and ``with db.write() as txn`` commits when the block ends and
aborts when it raises. Every collection is reached through a transaction,
by its class.
"""

from __future__ import annotations

import inspect
import os
import threading
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from types import TracebackType
from typing import TYPE_CHECKING, Any, Generic, Literal, TypeAlias, TypeVar, overload

from . import _native
from ._errors import DaruError, invalid
from ._native import NativeDatabase
from ._query import Condition, Prepared, Query, parse
from ._schema import Migration, Resolved, Schema

if TYPE_CHECKING:
    from typing import Self

    from ._async import AsyncReadScope, AsyncWriteScope

__all__ = [
    "BackupReport",
    "CheckProblem",
    "CheckReport",
    "CompactReport",
    "Database",
    "Durability",
    "Key",
    "Migrating",
    "PasswordHashing",
    "QueryInput",
    "ReadCollection",
    "ReadTransaction",
    "SalvageReport",
    "WriteCollection",
    "WriteTransaction",
]

T = TypeVar("T")

Key: TypeAlias = int | str | bytes
"""A primary key: an int, a str or bytes."""

Durability: TypeAlias = Literal["sync", "deferred"]
"""How a write commits: ``"sync"`` waits for the disk, ``"deferred"`` does not."""

QueryInput: TypeAlias = "Query | Condition | str | Prepared[Any] | None"
"""What ``find``, ``find_one`` and ``count`` take."""

FileKey: TypeAlias = tuple[int, int]


@dataclass(frozen=True)
class PasswordHashing:
    """What hashing a password costs, as Argon2id counts it. The default takes
    tens of milliseconds and fits the memory of a mobile app extension."""

    memory_kib: int = 19456
    """Memory, in KiB, up to 1 GiB."""
    iterations: int = 2
    parallelism: int = 1

    def spec(self) -> tuple[int, int, int]:
        return (self.memory_kib, self.iterations, self.parallelism)


@dataclass(frozen=True)
class CheckProblem:
    """One thing the integrity check found wrong."""

    page: int | None
    """The page the problem is in, when it is in one page."""
    tree: str | None
    """The tree or the collection it was found in, when it was found in one."""
    message: str
    """What is wrong."""


@dataclass(frozen=True)
class CheckReport:
    """What ``Database.check`` found: the commit it checked and every problem."""

    ok: bool
    """Whether the check found nothing wrong."""
    commit_id: int
    """The transaction id of the commit checked, the one published when it began."""
    page_count: int
    """The pages that commit counts, the header page included."""
    pages_checked: int
    """The pages read and verified."""
    objects_checked: int
    """The objects read and checked against their indexes."""
    problems: tuple[CheckProblem, ...]
    """Every problem found, in the order found."""


@dataclass(frozen=True)
class BackupReport:
    """What ``Database.backup`` wrote."""

    commit_id: int
    """The transaction id of the commit copied, the one published when the backup began."""
    trees: int
    """The trees copied, the engine's own included."""
    entries: int
    """The entries copied."""
    bytes: int
    """The size of the new file, in bytes."""


@dataclass(frozen=True)
class CompactReport:
    """What ``Database.compact`` did."""

    bytes_before: int
    """The size of the file before, in bytes."""
    bytes_after: int
    """The size of the file after, in bytes."""
    pages_moved: int
    """The pages moved out of the file's end."""


@dataclass(frozen=True)
class SalvageReport:
    """What ``Database.salvage`` rescued, and what it could not."""

    whole: bool
    """Whether the new file holds exactly the commit salvage started from."""
    commit_id: int | None
    """The commit salvage started from, or None when no commit record could be used."""
    pages_scanned: int
    """The pages of the file read, the header page left out."""
    pages_damaged: int
    """The pages that failed their check, other than pages never written."""
    pages_unread: int
    """The pages of the commit that could not be read."""
    entries_recovered: int
    """The entries taken from older versions of the pages that could not be read."""
    values_lost: int
    """The keys left out because no version of their value could be read."""
    objects_dropped: int
    """The objects left out."""
    trees: int
    """The trees of the new file, the engine's own included."""
    entries: int
    """The entries of the new file, the indexes' included."""
    bytes: int
    """The size of the new file, in bytes."""


def check_report(report: dict[str, Any]) -> CheckReport:
    problems = tuple(CheckProblem(**problem) for problem in report.pop("problems"))

    return CheckReport(problems=problems, **report)


def native_query(
    query: QueryInput, resolved: Resolved, schema: Schema | None
) -> _native.NativeQuery:
    """The engine's query for what ``find`` was given."""
    if query is None:
        return _EVERY.compile(resolved, schema)

    if isinstance(query, Query):
        return query.compile(resolved, schema)

    if isinstance(query, Condition):
        return query.query().compile(resolved, schema)

    if isinstance(query, str):
        return parse(query)

    if isinstance(query, Prepared):
        if query.collection != resolved.name:
            raise DaruError(
                "INVALID_QUERY",
                f"the query was prepared on {query.collection}, not {resolved.name}",
            )

        return query._native

    raise DaruError("INVALID_QUERY", f"a query is a Query, a condition or text, not {query!r}")


_EVERY = Query()

# The files that a write transaction of this thread holds, so that a second
# one fails at once rather than wait for itself until the busy timeout.
_writing = threading.local()


def _held(file_key: FileKey) -> set[FileKey]:
    held: set[FileKey] | None = getattr(_writing, "files", None)

    if held is None:
        held = set()
        _writing.files = held

    return held


def file_key_of(path: str) -> FileKey:
    """What identifies the file at ``path``, however it is reached: the device
    and the inode, which Python gives on Windows as the volume serial number
    and the file index."""
    status = os.stat(path)

    return (status.st_dev, status.st_ino)


class ReadCollection(Generic[T]):
    """A collection of a transaction, for reading its objects."""

    __slots__ = ("_resolved", "_schema", "_txn")

    def __init__(self, txn: _native.NativeTransaction, resolved: Resolved, schema: Schema) -> None:
        self._txn = txn
        self._resolved = resolved
        self._schema = schema

    @property
    def name(self) -> str:
        """The collection's name."""
        return self._resolved.name

    def __repr__(self) -> str:
        return f"<{type(self).__name__} {self.name!r}>"

    def get(self, key: Key) -> T | None:
        """The object whose primary key is ``key``, or None."""
        found: T | None = self._txn.get(self._resolved.name, self._resolved.layout, key)

        return found

    def find(self, query: QueryInput = None, /, *parameters: object) -> list[T]:
        """The objects a query finds, in its order: every object without one.

        ``query`` is a ``Query``, a condition, a prepared query, or text in
        the query language, with ``parameters`` for its ``$0``, ``$1`` and on.
        """
        found: list[T] = self._txn.find(
            self._resolved.name,
            self._resolved.layout,
            native_query(query, self._resolved, self._schema),
            parameters,
        )

        return found

    def find_one(self, query: QueryInput = None, /, *parameters: object) -> T | None:
        """The first object a query finds, or None."""
        found: T | None = self._txn.find_one(
            self._resolved.name,
            self._resolved.layout,
            native_query(query, self._resolved, self._schema),
            parameters,
        )

        return found

    def count(self, query: QueryInput = None, /, *parameters: object) -> int:
        """How many objects a query finds, after its offset and within its limit."""
        return self._txn.count(
            self._resolved.name, native_query(query, self._resolved, self._schema), parameters
        )


class WriteCollection(ReadCollection[T]):
    """A collection of a write transaction, for reading and writing its objects."""

    __slots__ = ()

    def insert(self, obj: T) -> Key:
        """Inserts ``obj`` and returns its primary key. A key or a unique value
        already taken is ``DUPLICATE_KEY``."""
        key: Key = self._txn.insert(self._resolved.name, self._resolved.layout, obj)

        return key

    def insert_many(self, objects: Iterable[T]) -> list[Key]:
        """Inserts ``objects`` in one call into the engine and returns their
        keys. A refused object stops the batch with its error, and the objects
        before it stay inserted in the transaction."""
        keys: list[Key] = self._txn.insert_many(self._resolved.name, self._resolved.layout, objects)

        return keys

    def put(self, obj: T) -> Key:
        """Inserts ``obj``, or replaces the object with its key, and returns its key."""
        key: Key = self._txn.put(self._resolved.name, self._resolved.layout, obj)

        return key

    def put_many(self, objects: Iterable[T]) -> list[Key]:
        """``put`` of each of ``objects``, in one call, as ``insert_many`` inserts them."""
        keys: list[Key] = self._txn.put_many(self._resolved.name, self._resolved.layout, objects)

        return keys

    def update(self, key: Key, /, **changes: object) -> bool:
        """Sets the fields ``changes`` names in the object whose primary key is
        ``key``, and says whether there was one; the rest of it stays.

        ``None`` makes an optional field None and gives a field with a default
        its default. An embedded object or a list is replaced whole.
        """
        return self._txn.update(self._resolved.name, self._resolved.layout, key, changes)

    def delete(self, key: Key) -> bool:
        """Deletes the object whose primary key is ``key``, and says whether there was one."""
        return self._txn.delete(self._resolved.name, key)


class ReadTransaction:
    """A read transaction: one commit, for as long as its block runs."""

    __slots__ = ("_native", "_schema")

    def __init__(self, native: _native.NativeTransaction, schema: Schema | None) -> None:
        self._native = native
        self._schema = schema

    def _resolved(self, collection: type[Any] | str) -> tuple[Resolved, Schema]:
        if self._schema is None:
            raise invalid("the database was opened without a schema, so it has no collections")

        return self._schema.resolved(collection), self._schema

    @overload
    def collection(self, collection: type[T]) -> ReadCollection[T]: ...

    @overload
    def collection(self, collection: str) -> ReadCollection[Any]: ...

    def collection(self, collection: type[Any] | str) -> ReadCollection[Any]:
        """The collection of the class ``collection``, or of that name."""
        resolved, schema = self._resolved(collection)

        return ReadCollection(self._native, resolved, schema)


class WriteTransaction(ReadTransaction):
    """A write transaction: changes that commit together when its block ends."""

    __slots__ = ()

    @overload
    def collection(self, collection: type[T]) -> WriteCollection[T]: ...

    @overload
    def collection(self, collection: str) -> WriteCollection[Any]: ...

    def collection(self, collection: type[Any] | str) -> WriteCollection[Any]:
        """The collection of the class ``collection``, or of that name."""
        resolved, schema = self._resolved(collection)

        return WriteCollection(self._native, resolved, schema)


class Migrating(WriteTransaction):
    """The write transaction of a migration, as a migration's ``run`` gets it:
    the collections of the new schema, and the objects as the schema before
    the migration read them."""

    __slots__ = ()

    @property
    def previous_version(self) -> int:
        """The schema version the file held before the migration."""
        return self._native.previous_version

    @property
    def version(self) -> int:
        """The version the migration leads to."""
        return self._native.version

    def previous(self, collection: str, key: Key) -> dict[str, Any] | None:
        """The object of ``collection``, named as before the migration, as the
        old schema reads it: a ``dict`` by the names the old schema stores.
        Read an object this way before writing it: a written object keeps only
        the new schema's fields."""
        found: dict[str, Any] | None = self._native.previous(collection, key)

        return found

    def previous_keys(self, collection: str) -> list[Key]:
        """The keys of every object of ``collection``, named as before the migration."""
        keys: list[Key] = self._native.previous_keys(collection)

        return keys


class _ReadScope:
    """``with db.read() as txn``."""

    __slots__ = ("_db", "_txn")

    def __init__(self, db: Database) -> None:
        self._db = db
        self._txn: _native.NativeTransaction | None = None

    def __enter__(self) -> ReadTransaction:
        self._txn = self._db._handle().begin_read()

        return ReadTransaction(self._txn, self._db.schema)

    def __exit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        if self._txn is not None:
            self._txn.end()
            self._txn = None


class _WriteScope:
    """``with db.write() as txn``."""

    __slots__ = ("_db", "_deferred", "_txn")

    def __init__(self, db: Database, durability: Durability) -> None:
        if durability not in ("sync", "deferred"):
            raise invalid(f"durability is 'sync' or 'deferred', not {durability!r}")

        self._db = db
        self._deferred = durability == "deferred"
        self._txn: _native.NativeTransaction | None = None

    def __enter__(self) -> WriteTransaction:
        db = self._db
        held = _held(db._file_key)

        if db._file_key in held:
            raise invalid("write transactions on one file do not nest")

        db._refuse_while_async_writes("write")
        held.add(db._file_key)

        try:
            self._txn = db._handle().begin_write()
        except BaseException:
            held.discard(db._file_key)
            raise

        return WriteTransaction(self._txn, db.schema)

    def __exit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        txn = self._txn
        self._txn = None
        _held(self._db._file_key).discard(self._db._file_key)

        if txn is None:
            return

        if kind is None:
            txn.commit(self._deferred)
        else:
            txn.end()


def _run_migrations(
    native: _native.NativeTransaction, schema: Schema | None, migrations: Sequence[Migration]
) -> _native.NativeDatabase:
    """Runs a migration that is under way to its end, with each step's function."""
    steps = {migration.version: migration for migration in migrations}
    migrating = Migrating(native, schema)

    try:
        while (version := native.next_step()) is not None:
            step = steps.get(version)

            if step is not None and step.run is not None:
                result = step.run(migrating)  # type: ignore[arg-type]

                if inspect.isawaitable(result):
                    close = getattr(result, "close", None)

                    if close is not None:
                        close()

                    raise invalid(
                        f"the migration to version {version} is a coroutine function, "
                        "which Database.open_async runs"
                    )

        return native.finish()
    except BaseException:
        native.end()
        raise


def check_migrations(migrations: Sequence[Migration]) -> None:
    """Refuses what is not a migration, and two migrations to one version."""
    seen: set[int] = set()

    for migration in migrations:
        if not isinstance(migration, Migration):
            raise invalid(f"a migration is a Migration, not {migration!r}")

        if migration.version in seen:
            raise invalid(f"two migrations lead to version {migration.version}")

        seen.add(migration.version)


class Database:
    """An open database. There is no constructor: use ``Database.open``.

    Every method that uses the file has an asynchronous twin whose name ends
    in ``_async``, which does the engine's work on a thread of the package's
    pool, so that an event loop never waits for the disk or for another
    process's writer. A database is a context manager that closes it.
    """

    __slots__ = ("_file_key", "_native", "path", "schema")

    path: str
    """The path the database was opened at. Still readable after ``close``."""
    schema: Schema | None
    """The schema the database was opened with, or None."""
    _native: NativeDatabase
    _file_key: FileKey

    def __init__(self) -> None:
        raise TypeError("a Database is opened with Database.open")

    @classmethod
    def _of(cls, native: NativeDatabase, path: str, schema: Schema | None) -> Database:
        db = object.__new__(cls)
        db._native = native
        db.path = path
        db.schema = schema
        db._file_key = file_key_of(path)

        return db

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
    ) -> Database:
        """Opens the database at ``path``, creating it if nothing exists there,
        and stores, checks or migrates its schema.

        - ``create``: whether to create the database when nothing exists at
          the path. Without it, a missing file is ``NOT_FOUND``.
        - ``page_size``: the page size of a new database, a power of two from
          4096 to 65536. 4096 by default.
        - ``busy_timeout``: how long, in seconds, opening and a write wait for
          another process's writer before failing with ``BUSY``. 5 by default.
        - ``cache_size``: how much memory the page cache may take, in bytes.
          32 MiB by default.
        - ``key`` or ``password``: encrypts a new database, or opens an
          encrypted one. A key is 32 bytes; a password is hashed with Argon2id
          at the cost ``password_hashing`` sets.
        """
        location = os.fspath(path)
        check_migrations(migrations)
        native = _native.open(
            location,
            create=create,
            page_size=page_size,
            busy_timeout=busy_timeout,
            cache_size=cache_size,
            schema=None if schema is None else schema._native,
            migrations=[migration.spec() for migration in migrations],
            key=key,
            password=password,
            password_hashing=None if password_hashing is None else password_hashing.spec(),
        )

        if isinstance(native, _native.NativeTransaction):
            native = _run_migrations(native, schema, migrations)

        return cls._of(native, location, schema)

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
    ) -> Database:
        """``open`` on the package's thread pool. A migration's ``run`` may be
        a coroutine function, given an ``AsyncMigrating``."""
        from ._async import open_async

        return await open_async(
            cls,
            os.fspath(path),
            schema=schema,
            migrations=migrations,
            create=create,
            page_size=page_size,
            busy_timeout=busy_timeout,
            cache_size=cache_size,
            key=key,
            password=password,
            password_hashing=password_hashing,
        )

    @staticmethod
    def salvage(
        source: str | os.PathLike[str],
        target: str | os.PathLike[str],
        *,
        key: bytes | bytearray | memoryview | None = None,
        password: str | bytes | bytearray | None = None,
        busy_timeout: float | None = None,
    ) -> SalvageReport:
        """Rescues what it can of the damaged database at ``source`` into a new
        database at ``target``, and reports what it rescued and what it could
        not. It reads the file page by page, so it works on a file that does
        not open, and builds every index again, so the new file passes the
        integrity check.

        It needs the file alone: a file open in this process or another is
        ``BUSY``. It never replaces a file: a ``target`` that is taken is
        ``INVALID_ARGUMENT``.
        """
        report = _native.salvage(
            os.fspath(source),
            os.fspath(target),
            key=key,
            password=password,
            busy_timeout=busy_timeout,
        )

        return SalvageReport(**report)

    @staticmethod
    async def salvage_async(
        source: str | os.PathLike[str],
        target: str | os.PathLike[str],
        *,
        key: bytes | bytearray | memoryview | None = None,
        password: str | bytes | bytearray | None = None,
        busy_timeout: float | None = None,
    ) -> SalvageReport:
        """``salvage`` on the package's thread pool."""
        from ._async import run

        return await run(
            lambda: Database.salvage(
                source, target, key=key, password=password, busy_timeout=busy_timeout
            )
        )

    def _handle(self) -> NativeDatabase:
        return self._native

    def _refuse_while_async_writes(self, what: str) -> None:
        from ._async import refuse_on_loop

        refuse_on_loop(self._file_key, what)

    def __repr__(self) -> str:
        state = "open" if self.is_open else "closed"

        return f"<Database {self.path!r} {state}>"

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        self.close()

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        await self.close_async()

    @property
    def is_open(self) -> bool:
        """Whether ``close`` has not been called."""
        return self._native.is_open

    @property
    def page_size(self) -> int:
        """The size of every page in the file, in bytes."""
        return self._native.page_size

    @property
    def format_version(self) -> int:
        """The file format version recorded in the file."""
        return self._native.format_version

    @property
    def is_encrypted(self) -> bool:
        """Whether the file is encrypted."""
        return self._native.is_encrypted

    @property
    def schema_version(self) -> int | None:
        """The schema version the file holds, or None without a schema."""
        return None if self.schema is None else self.schema.version

    def prepare(self, collection: type[T] | str, query: Query | Condition | str) -> Prepared[T]:
        """Prepares a query on ``collection``: text in the query language with
        ``$0``, ``$1`` and on, or a query built with ``param`` in place of
        values. It is compiled once here, and each ``find``, ``find_one`` or
        ``count`` gives its parameters' values."""
        if self.schema is None:
            raise invalid("the database was opened without a schema, so it has no collections")

        resolved = self.schema.resolved(collection)

        return Prepared(resolved.name, native_query(query, resolved, self.schema))

    def read(self) -> _ReadScope:
        """A read transaction: ``with db.read() as txn``. It sees the last
        commit, and nothing committed after it, until the block ends."""
        return _ReadScope(self)

    def write(self, *, durability: Durability = "sync") -> _WriteScope:
        """The write transaction: ``with db.write() as txn``. It commits when the
        block ends and aborts when the block raises. It waits for another
        writer, in this process or another, up to the busy timeout.

        ``durability="deferred"`` commits without waiting for the disk:
        readers see the commit at once, a crash of the process loses none of
        it, and it becomes durable at the next sync or within a second.
        """
        return _WriteScope(self, durability)

    def read_async(self) -> AsyncReadScope:
        """A read transaction for ``async with``, whose operations are awaited."""
        from ._async import AsyncReadScope

        return AsyncReadScope(self)

    def write_async(self, *, durability: Durability = "sync") -> AsyncWriteScope:
        """The write transaction for ``async with``. This process's
        asynchronous writes on one file run one after another."""
        from ._async import AsyncWriteScope

        return AsyncWriteScope(self, durability)

    def check(self) -> CheckReport:
        """Checks the published commit completely: every page against its check,
        the order of every key, every count, that every page is used, free or
        retained exactly once, and every object against its indexes. It
        reports every problem rather than raising, and reads while other
        handles and processes write."""
        return check_report(self._native.check())

    def backup(
        self,
        path: str | os.PathLike[str],
        *,
        key: bytes | bytearray | memoryview | None = None,
        password: str | bytes | bytearray | None = None,
        password_hashing: PasswordHashing | None = None,
    ) -> BackupReport:
        """Writes a copy of the published commit to a new file at ``path``,
        while other handles and processes may write. The copy holds no free
        space and opens with the same key or password, or, with ``key`` or
        ``password``, is encrypted under a new data key that only they open.
        It never replaces a file: a ``path`` that is taken is
        ``INVALID_ARGUMENT``."""
        report = self._native.backup(
            os.fspath(path),
            key=key,
            password=password,
            password_hashing=None if password_hashing is None else password_hashing.spec(),
        )

        return BackupReport(**report)

    def compact(self) -> CompactReport:
        """Makes the file smaller in place: its end moves into free pages nearer
        its start and goes back to the file system. It writes, so it waits for
        the writer."""
        self._refuse_while_async_writes("compact")

        return CompactReport(**self._native.compact())

    def set_key(self, key: bytes | bytearray | memoryview) -> None:
        """Changes the key of an encrypted database to ``key``, 32 bytes. No page
        is encrypted again, and once it returns, the old key or password no
        longer opens the file. A plain database is ``INVALID_ARGUMENT``."""
        self._refuse_while_async_writes("set_key")
        self._native.set_key(key)

    def set_password(self, password: str | bytes | bytearray) -> None:
        """Changes the key of an encrypted database to one derived from
        ``password``, at the hashing cost the database was opened with."""
        self._refuse_while_async_writes("set_password")
        self._native.set_password(password)

    def sync(self) -> None:
        """Makes every commit durable, deferred ones included, whichever process
        made them."""
        self._refuse_while_async_writes("sync")
        self._native.sync()

    def close(self) -> None:
        """Makes deferred commits durable and closes the database. Closing one
        that is closed does nothing."""
        if self._native.is_open:
            self._refuse_while_async_writes("close")

        self._native.close()

    async def check_async(self) -> CheckReport:
        """``check`` on the package's thread pool."""
        from ._async import run

        return await run(self.check)

    async def backup_async(
        self,
        path: str | os.PathLike[str],
        *,
        key: bytes | bytearray | memoryview | None = None,
        password: str | bytes | bytearray | None = None,
        password_hashing: PasswordHashing | None = None,
    ) -> BackupReport:
        """``backup`` on the package's thread pool."""
        from ._async import run

        return await run(
            lambda: self.backup(path, key=key, password=password, password_hashing=password_hashing)
        )

    async def compact_async(self) -> CompactReport:
        """``compact`` on the package's thread pool, after this process's writes on the file."""
        from ._async import after_writes

        return await after_writes(self, lambda: CompactReport(**self._native.compact()))

    async def set_key_async(self, key: bytes | bytearray | memoryview) -> None:
        """``set_key`` on the package's thread pool, after this process's writes on the file."""
        from ._async import after_writes

        # The caller's buffer, which it can wipe once this returns: the native
        # module copies it when the call runs, into memory it wipes itself.
        await after_writes(self, lambda: self._native.set_key(key))

    async def set_password_async(self, password: str | bytes | bytearray) -> None:
        """``set_password`` on the package's thread pool, after this process's writes."""
        from ._async import after_writes

        await after_writes(self, lambda: self._native.set_password(password))

    async def sync_async(self) -> None:
        """``sync`` on the package's thread pool, after this process's writes on the file."""
        from ._async import after_writes

        await after_writes(self, self._native.sync)

    async def close_async(self) -> None:
        """``close`` on the package's thread pool, after this process's writes on the file."""
        from ._async import after_writes

        await after_writes(self, self._native.close)
