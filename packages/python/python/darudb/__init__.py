"""DaruDB, an embedded database that keeps an application's data in one local
file.

Collections are classes::

    import darudb
    from darudb import F, field

    @darudb.collection("users")
    class User:
        id: int | None = None
        name: str
        age: int = field(default=0, index=True)

    db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))

    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice", age=31))

    with db.read() as txn:
        adults = txn.collection(User).find(darudb.where(F.age >= 18).sort_by(F.age))

The engine is written in Rust, and this package is its Python binding: the
same file reads the same from Rust, Node.js, Dart and Python.
"""

from __future__ import annotations

from . import _native
from ._async import (
    AsyncMigrating,
    AsyncReadCollection,
    AsyncReadTransaction,
    AsyncWriteCollection,
    AsyncWriteTransaction,
)
from ._database import (
    BackupReport,
    CheckProblem,
    CheckReport,
    CompactReport,
    Database,
    Durability,
    Key,
    Migrating,
    PasswordHashing,
    QueryInput,
    ReadCollection,
    ReadTransaction,
    SalvageReport,
    WriteCollection,
    WriteTransaction,
)
from ._errors import DaruError
from ._query import Condition, F, FieldRef, Param, Prepared, Query, param, where
from ._schema import Migration, Schema, collection, embedded, field

__all__ = [
    "ENGINE_VERSION",
    "FORMAT_VERSION",
    "AsyncMigrating",
    "AsyncReadCollection",
    "AsyncReadTransaction",
    "AsyncWriteCollection",
    "AsyncWriteTransaction",
    "BackupReport",
    "CheckProblem",
    "CheckReport",
    "CompactReport",
    "Condition",
    "DaruError",
    "Database",
    "Durability",
    "F",
    "FieldRef",
    "Key",
    "Migrating",
    "Migration",
    "Param",
    "PasswordHashing",
    "Prepared",
    "Query",
    "QueryInput",
    "ReadCollection",
    "ReadTransaction",
    "SalvageReport",
    "Schema",
    "WriteCollection",
    "WriteTransaction",
    "__version__",
    "collection",
    "embedded",
    "field",
    "param",
    "where",
]

__version__ = "1.0.0"
"""The version of this package."""

ENGINE_VERSION: str = _native.engine_version()
"""The version of the engine inside the package."""

FORMAT_VERSION: int = _native.FORMAT_VERSION
"""The file format version this package reads and writes."""
