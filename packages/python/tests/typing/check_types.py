"""What a type checker infers through the package's API.

mypy checks this file with the package (``pyproject.toml``); it is never
run. A line that stops type-checking as written here fails the check.
"""

from __future__ import annotations

from typing import Any, assert_type

import darudb
from darudb import F, field, param, where


@darudb.embedded
class Address:
    city: str
    zip: str | None = field(default=None, name="postcode")


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    age: int = field(default=0, index=True)
    tags: list[str] = field(default_factory=list)
    address: Address | None = None


@darudb.collection
class Post:
    slug: str = field(primary_key=True)
    author: int = field(link=User)


schema = darudb.Schema(1, [User, Post])


def reads(db: darudb.Database) -> None:
    user = User(name="Alice", age=3)
    assert_type(user.age, int)

    with db.read() as txn:
        users = txn.collection(User)
        assert_type(users, darudb.ReadCollection[User])
        assert_type(users.get(1), User | None)
        assert_type(users.find(), list[User])
        assert_type(users.find(where(F.age >= 18).sort_by(F.age).limit(3)), list[User])
        assert_type(users.find("age >= $0", 18), list[User])
        assert_type(users.find_one(F.name == "Alice"), User | None)
        assert_type(users.count(), int)
        assert_type(txn.collection("users"), darudb.ReadCollection[Any])

    prepared = db.prepare(User, where(F.age >= param(0)))
    assert_type(prepared, darudb.Prepared[User])


def writes(db: darudb.Database) -> None:
    with db.write(durability="deferred") as txn:
        users = txn.collection(User)
        assert_type(users, darudb.WriteCollection[User])
        assert_type(users.insert(User(name="Bob")), darudb.Key)
        assert_type(users.insert_many([User(name="Carol")]), list[darudb.Key])
        assert_type(users.update(1, age=4), bool)
        assert_type(users.delete(1), bool)
        txn.collection(Post).put(Post(slug="hi", author=1))


async def async_api(db: darudb.Database) -> None:
    async with db.write_async() as txn:
        users = txn.collection(User)
        assert_type(users, darudb.AsyncWriteCollection[User])
        assert_type(await users.insert(User(name="Dave")), darudb.Key)

    async with db.read_async() as read:
        assert_type(await read.collection(User).get(1), User | None)

    assert_type(await db.check_async(), darudb.CheckReport)


def opening() -> None:
    with darudb.Database.open("app.darudb", schema=schema) as db:
        assert_type(db, darudb.Database)
        assert_type(db.schema_version, int | None)
        assert_type(db.check(), darudb.CheckReport)


def migration(migrating: darudb.Migrating) -> None:
    assert_type(migrating.previous("people", 1), dict[str, Any] | None)
    assert_type(migrating.collection(User), darudb.WriteCollection[User])


steps = [darudb.Migration(2, rename_fields=[("users", "fullName", "name")], run=migration)]
