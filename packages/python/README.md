# darudb

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/jooy2/darudb/blob/main/LICENSE)

The Python package of [DaruDB](https://darudb.cdget.com), an embedded database that keeps an application's data in one local file. It is a native extension over the DaruDB engine, which is written in Rust, so the same file reads the same from Python, Rust, Node.js and Dart.

## Installation

```bash
pip install darudb
```

The package needs CPython 3.11 or later. Wheels are built for Linux (glibc 2.17 or later, and musl), macOS and Windows, on x86-64 and ARM64, and for the free-threaded build of Python 3.14 as well, so installing it needs no Rust toolchain.

## Usage

Each collection is a class. The decorator makes it a frozen, keyword-only dataclass, and its annotations are its fields' types; `field` adds an index, a primary key, a link or a default:

```python
import darudb
from darudb import F, field, where


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))

with db.write() as txn:
    txn.collection(User).insert_many([User(name="Alice", age=31), User(name="Bob", age=17)])

with db.read() as txn:
    users = txn.collection(User)
    adults = users.find(where(F.age >= 18).sort_by(F.age, descending=True).limit(10))
    same = users.find("age >= $0 SORT BY age DESC LIMIT 10", 18)

db.close()
```

A `write` block commits when it ends and aborts when it raises, and `db.write(durability="deferred")` commits without waiting for the disk. An object read is an instance of its class. Every failure is a `darudb.DaruError` whose `code` is the engine's, the same string in every language DaruDB ships to.

The engine releases the GIL while it works, so other threads run while one waits for the disk. Every operation also has an asynchronous twin for `asyncio`, which runs the engine's work on a thread of the package's pool:

```python
db = await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User]))

async with db.write_async() as txn:
    await txn.collection(User).insert(User(name="Carol"))

async with db.read_async() as txn:
    count = await txn.collection(User).count()

await db.close_async()
```

Raising the schema's version migrates the file when it opens, with renames, deletions and a Python function of your own. Encryption, the integrity check, backup, compaction and salvage are all there; [the guide](https://darudb.cdget.com/guide/getting-started) has the details.

## License

[MIT](https://github.com/jooy2/darudb/blob/main/LICENSE) © [CDGet](https://cdget.com)
