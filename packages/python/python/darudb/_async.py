"""The asynchronous API: the same operations, awaited, on the package's pool.

Every operation that uses the file runs on a thread of one pool the package
keeps, where the native module releases the GIL for the engine's work, so an
event loop never waits for the disk or for another process's writer.

Two rules keep the pool from waiting for itself:

- An event loop's asynchronous writes on one file run one after another,
  queued here on the loop rather than on the pool: a write waiting there for
  another one of this process could take every thread while the one it waits
  for needs a thread to go on. ``sync_async``, ``close_async``,
  ``compact_async`` and the key changes queue with them. One of them awaited
  inside a write on the file, or in a task made inside it, would wait for the
  write it is part of, so it is refused with ``INVALID_ARGUMENT``: a context
  variable carries the files a write holds into every task made inside it.
- A synchronous ``write``, ``sync``, ``close``, ``compact`` or key change on
  the thread of an event loop whose asynchronous write holds the file would
  wait for a write that needs that loop, so it is refused with
  ``INVALID_ARGUMENT``.

The operations of one transaction run in the order they were called, one at
a time.
"""

from __future__ import annotations

import asyncio
import contextvars
import inspect
import os
import threading
import weakref
from collections import Counter
from collections.abc import Callable, Iterable, Sequence
from concurrent.futures import ThreadPoolExecutor
from functools import partial
from types import TracebackType
from typing import TYPE_CHECKING, Any, Generic, TypeVar, overload

from . import _native
from ._database import (
    Durability,
    FileKey,
    Key,
    QueryInput,
    _held,
    check_migrations,
    file_key_of,
    native_query,
)
from ._errors import invalid
from ._schema import Migration, Resolved, Schema

if TYPE_CHECKING:
    from ._database import Database, PasswordHashing

__all__ = [
    "AsyncMigrating",
    "AsyncReadCollection",
    "AsyncReadScope",
    "AsyncReadTransaction",
    "AsyncWriteCollection",
    "AsyncWriteScope",
    "AsyncWriteTransaction",
]

T = TypeVar("T")
R = TypeVar("R")

_pool_lock = threading.Lock()
_pool: ThreadPoolExecutor | None = None


def _executor() -> ThreadPoolExecutor:
    global _pool

    with _pool_lock:
        if _pool is None:
            # The pool grows only as calls wait, up to this many threads.
            _pool = ThreadPoolExecutor(
                max_workers=min(32, (os.cpu_count() or 1) + 4),
                thread_name_prefix="darudb",
            )

        return _pool


async def run(work: Callable[[], R]) -> R:
    """``work`` on the package's pool."""
    return await asyncio.get_running_loop().run_in_executor(_executor(), work)


# Each event loop's queue of asynchronous writes, a lock per file.
_queues: weakref.WeakKeyDictionary[asyncio.AbstractEventLoop, dict[FileKey, asyncio.Lock]] = (
    weakref.WeakKeyDictionary()
)
# The files the asynchronous writes around the running code hold. Each task
# made inside a write starts with a copy, so a task of its own sees them too.
_inside: contextvars.ContextVar[frozenset[FileKey]] = contextvars.ContextVar(
    "darudb_inside", default=frozenset()
)
# The threads whose event loop holds a file for an asynchronous write.
_holders: Counter[tuple[FileKey, int]] = Counter()
_holders_lock = threading.Lock()


def _queue(file_key: FileKey) -> asyncio.Lock:
    loop = asyncio.get_running_loop()
    queues = _queues.setdefault(loop, {})
    lock = queues.get(file_key)

    if lock is None:
        lock = queues[file_key] = asyncio.Lock()

    return lock


class _Hold:
    """This loop's turn on a file, from the queue, until it is released."""

    __slots__ = ("_file_key", "_lock", "_thread")

    def __init__(self, file_key: FileKey) -> None:
        self._file_key = file_key
        self._lock = _queue(file_key)
        self._thread = threading.get_ident()

    async def take(self) -> None:
        if self._file_key in _inside.get():
            raise invalid(
                "the asynchronous write that holds the file is waiting for this call to finish: "
                "write transactions on one file do not nest"
            )

        await self._lock.acquire()

        with _holders_lock:
            _holders[(self._file_key, self._thread)] += 1

    def release(self) -> None:
        with _holders_lock:
            _holders[(self._file_key, self._thread)] -= 1

            if _holders[(self._file_key, self._thread)] <= 0:
                del _holders[(self._file_key, self._thread)]

        self._lock.release()


def refuse_on_loop(file_key: FileKey, what: str) -> None:
    """Refuses ``what`` on a thread whose event loop holds the file."""
    with _holders_lock:
        held = (file_key, threading.get_ident()) in _holders

    if held:
        raise invalid(
            f"{what} would wait for the asynchronous write that holds the file, which needs "
            f"this event loop: await that write first, or use {what}_async"
        )


async def after_writes(db: Database, work: Callable[[], R]) -> R:
    """``work`` on the pool, in its turn after this process's writes on the file."""
    hold = _Hold(db._file_key)

    await hold.take()

    try:
        return await run(work)
    finally:
        hold.release()


class _Ordered:
    """Runs a transaction's operations on the pool one at a time, in the
    order they were called."""

    __slots__ = ("_lock",)

    def __init__(self) -> None:
        self._lock = asyncio.Lock()

    async def call(self, work: Callable[..., R], *args: Any) -> R:
        async with self._lock:
            return await run(partial(work, *args))


class AsyncReadCollection(Generic[T]):
    """A collection of an asynchronous transaction, for reading its objects."""

    __slots__ = ("_order", "_resolved", "_schema", "_txn")

    def __init__(
        self,
        txn: _native.NativeTransaction,
        order: _Ordered,
        resolved: Resolved,
        schema: Schema,
    ) -> None:
        self._txn = txn
        self._order = order
        self._resolved = resolved
        self._schema = schema

    @property
    def name(self) -> str:
        """The collection's name."""
        return self._resolved.name

    def __repr__(self) -> str:
        return f"<{type(self).__name__} {self.name!r}>"

    async def get(self, key: Key) -> T | None:
        """The object whose primary key is ``key``, or None."""
        found: T | None = await self._order.call(
            self._txn.get, self._resolved.name, self._resolved.layout, key
        )

        return found

    async def find(self, query: QueryInput = None, /, *parameters: object) -> list[T]:
        """The objects a query finds, in its order: every object without one."""
        native = native_query(query, self._resolved, self._schema)
        found: list[T] = await self._order.call(
            self._txn.find, self._resolved.name, self._resolved.layout, native, parameters
        )

        return found

    async def find_one(self, query: QueryInput = None, /, *parameters: object) -> T | None:
        """The first object a query finds, or None."""
        native = native_query(query, self._resolved, self._schema)
        found: T | None = await self._order.call(
            self._txn.find_one, self._resolved.name, self._resolved.layout, native, parameters
        )

        return found

    async def count(self, query: QueryInput = None, /, *parameters: object) -> int:
        """How many objects a query finds, after its offset and within its limit."""
        native = native_query(query, self._resolved, self._schema)

        return await self._order.call(self._txn.count, self._resolved.name, native, parameters)


class AsyncWriteCollection(AsyncReadCollection[T]):
    """A collection of an asynchronous write transaction, for reading and writing."""

    __slots__ = ()

    async def insert(self, obj: T) -> Key:
        """Inserts ``obj`` and returns its primary key."""
        key: Key = await self._order.call(
            self._txn.insert, self._resolved.name, self._resolved.layout, obj
        )

        return key

    async def insert_many(self, objects: Iterable[T]) -> list[Key]:
        """Inserts ``objects`` in one call into the engine and returns their keys."""
        batch = list(objects)
        keys: list[Key] = await self._order.call(
            self._txn.insert_many, self._resolved.name, self._resolved.layout, batch
        )

        return keys

    async def put(self, obj: T) -> Key:
        """Inserts ``obj``, or replaces the object with its key, and returns its key."""
        key: Key = await self._order.call(
            self._txn.put, self._resolved.name, self._resolved.layout, obj
        )

        return key

    async def put_many(self, objects: Iterable[T]) -> list[Key]:
        """``put`` of each of ``objects``, in one call."""
        batch = list(objects)
        keys: list[Key] = await self._order.call(
            self._txn.put_many, self._resolved.name, self._resolved.layout, batch
        )

        return keys

    async def update(self, key: Key, /, **changes: object) -> bool:
        """Sets the fields ``changes`` names in the object whose primary key is
        ``key``; see ``WriteCollection.update``."""
        return await self._order.call(
            self._txn.update, self._resolved.name, self._resolved.layout, key, changes
        )

    async def delete(self, key: Key) -> bool:
        """Deletes the object whose primary key is ``key``, and says whether there was one."""
        return await self._order.call(self._txn.delete, self._resolved.name, key)


class AsyncReadTransaction:
    """An asynchronous read transaction: one commit, until its block ends."""

    __slots__ = ("_native", "_order", "_schema")

    def __init__(self, native: _native.NativeTransaction, schema: Schema | None) -> None:
        self._native = native
        self._schema = schema
        self._order = _Ordered()

    def _resolved(self, collection: type[Any] | str) -> tuple[Resolved, Schema]:
        if self._schema is None:
            raise invalid("the database was opened without a schema, so it has no collections")

        return self._schema.resolved(collection), self._schema

    @overload
    def collection(self, collection: type[T]) -> AsyncReadCollection[T]: ...

    @overload
    def collection(self, collection: str) -> AsyncReadCollection[Any]: ...

    def collection(self, collection: type[Any] | str) -> AsyncReadCollection[Any]:
        """The collection of the class ``collection``, or of that name."""
        resolved, schema = self._resolved(collection)

        return AsyncReadCollection(self._native, self._order, resolved, schema)

    async def _drain(self) -> None:
        """Waits for every operation called so far."""
        async with self._order._lock:
            pass


class AsyncWriteTransaction(AsyncReadTransaction):
    """An asynchronous write transaction: changes that commit together when its
    block ends, once every operation called has finished."""

    __slots__ = ()

    @overload
    def collection(self, collection: type[T]) -> AsyncWriteCollection[T]: ...

    @overload
    def collection(self, collection: str) -> AsyncWriteCollection[Any]: ...

    def collection(self, collection: type[Any] | str) -> AsyncWriteCollection[Any]:
        """The collection of the class ``collection``, or of that name."""
        resolved, schema = self._resolved(collection)

        return AsyncWriteCollection(self._native, self._order, resolved, schema)


class AsyncMigrating(AsyncWriteTransaction):
    """The write transaction of a migration, as a migration's ``run`` gets it
    from ``Database.open_async``."""

    __slots__ = ()

    @property
    def previous_version(self) -> int:
        """The schema version the file held before the migration."""
        return self._native.previous_version

    @property
    def version(self) -> int:
        """The version the migration leads to."""
        return self._native.version

    async def previous(self, collection: str, key: Key) -> dict[str, Any] | None:
        """``Migrating.previous``, awaited."""
        found: dict[str, Any] | None = await self._order.call(
            self._native.previous, collection, key
        )

        return found

    async def previous_keys(self, collection: str) -> list[Key]:
        """The keys of every object of ``collection``, named as before the migration."""
        keys: list[Key] = await self._order.call(self._native.previous_keys, collection)

        return keys


class AsyncReadScope:
    """``async with db.read_async() as txn``."""

    __slots__ = ("_db", "_txn")

    def __init__(self, db: Database) -> None:
        self._db = db
        self._txn: AsyncReadTransaction | None = None

    async def __aenter__(self) -> AsyncReadTransaction:
        # Beginning a read waits for no writer, so it runs here at once.
        self._txn = AsyncReadTransaction(self._db._handle().begin_read(), self._db.schema)

        return self._txn

    async def __aexit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        txn = self._txn
        self._txn = None

        if txn is not None:
            try:
                await txn._drain()
            finally:
                txn._native.end()


class AsyncWriteScope:
    """``async with db.write_async() as txn``."""

    __slots__ = ("_db", "_deferred", "_hold", "_token", "_txn")

    def __init__(self, db: Database, durability: Durability) -> None:
        if durability not in ("sync", "deferred"):
            raise invalid(f"durability is 'sync' or 'deferred', not {durability!r}")

        self._db = db
        self._deferred = durability == "deferred"
        self._hold: _Hold | None = None
        self._token: contextvars.Token[frozenset[FileKey]] | None = None
        self._txn: AsyncWriteTransaction | None = None

    async def __aenter__(self) -> AsyncWriteTransaction:
        db = self._db

        if db._file_key in _held(db._file_key):
            raise invalid("write transactions on one file do not nest")

        hold = _Hold(db._file_key)

        await hold.take()

        try:
            native = await run(db._handle().begin_write)
        except BaseException:
            hold.release()
            raise

        self._hold = hold
        self._token = _inside.set(_inside.get() | {db._file_key})
        self._txn = AsyncWriteTransaction(native, db.schema)

        return self._txn

    async def __aexit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        txn, hold, token = self._txn, self._hold, self._token
        self._txn = self._hold = self._token = None

        if txn is None or hold is None:
            return

        if token is not None:
            _inside.reset(token)

        try:
            await txn._drain()

            if kind is None:
                await run(partial(txn._native.commit, self._deferred))
            else:
                txn._native.end()
        except BaseException:
            txn._native.end()
            raise
        finally:
            hold.release()


async def _migrate(
    native: _native.NativeTransaction, schema: Schema | None, migrations: Sequence[Migration]
) -> _native.NativeDatabase:
    """Runs a migration that is under way to its end, awaiting each step's function."""
    steps = {migration.version: migration for migration in migrations}
    migrating = AsyncMigrating(native, schema)

    try:
        while (version := await run(native.next_step)) is not None:
            step = steps.get(version)

            if step is not None and step.run is not None:
                result: object = step.run(migrating)  # type: ignore[arg-type]

                if inspect.isawaitable(result):
                    await result

                await migrating._drain()

        return await run(native.finish)
    except BaseException:
        native.end()
        raise


async def open_async(
    cls: type[Database],
    location: str,
    *,
    schema: Schema | None,
    migrations: Sequence[Migration],
    create: bool,
    page_size: int | None,
    busy_timeout: float | None,
    cache_size: int | None,
    key: bytes | bytearray | memoryview | None,
    password: str | bytes | bytearray | None,
    password_hashing: PasswordHashing | None,
) -> Database:
    check_migrations(migrations)
    native = await run(
        partial(
            _native.open,
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
    )

    if isinstance(native, _native.NativeTransaction):
        # The migration holds the file as a write does.
        token = _inside.set(_inside.get() | {file_key_of(location)})

        try:
            native = await _migrate(native, schema, migrations)
        finally:
            _inside.reset(token)

    return cls._of(native, location, schema)
