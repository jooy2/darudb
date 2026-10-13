"""A file opened without a schema: the database works with the schema the file
holds, reaches its collections by name, reads and writes their objects as
``dict``s by the names the file stores, and describes the schema."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F, StoredField, StoredIndex, StoredType
from models import SCHEMA, Address, User, open_db


def with_users(path: Path) -> None:
    """A file at ``SCHEMA`` with two users, written through the classes."""
    with open_db(path) as db, db.write() as txn:
        txn.collection(User).insert_many(
            [
                User(
                    name="Alice",
                    email="alice@example.com",
                    age=31,
                    tags=["red"],
                    address=Address(city="Seoul", zip="04524"),
                ),
                User(name="Bob", age=17),
            ]
        )


def test_the_file_s_own_schema_is_described_by_the_names_it_stores(path: Path) -> None:
    with_users(path)

    with darudb.Database.open(path) as db:
        stored = db.stored_schema

        assert db.schema is None
        assert stored is not None
        assert db.schema_version == 1
        assert [collection.name for collection in stored.collections] == [
            "users",
            "posts",
            "blobs",
        ]

        users, posts, blobs = stored.collections
        fields = {field.name: field for field in users.fields}

        assert users.key == "id"
        assert users.auto_increment
        assert "id" not in fields
        assert fields["name"] == StoredField("name", StoredType("string"), optional=False)
        assert fields["email"].optional
        assert fields["age"].default == 0
        assert fields["rating"] == StoredField(
            "rating", StoredType("float"), optional=False, default=0.0
        )
        assert fields["tags"].type == StoredType("list", element=StoredType("string"))
        # An embedded object's fields go by the names the file stores.
        assert fields["address"].type == StoredType(
            "object",
            fields=(
                StoredField("city", StoredType("string"), optional=False),
                StoredField("postcode", StoredType("string"), optional=True),
            ),
        )
        assert set(users.indexes) == {
            StoredIndex("email", unique=True),
            StoredIndex("age", unique=False),
            StoredIndex("tags", unique=False),
        }
        assert posts.key == "slug"
        assert not posts.auto_increment
        assert posts.fields[1].type == StoredType("link", collection="users")
        assert blobs.fields[0].type == StoredType("bytes")

    # Still readable after the database closes, and the same for a database
    # that declares the schema.
    assert db.stored_schema == stored
    assert open_db(path).stored_schema == stored


def test_objects_are_read_and_written_as_dicts_with_the_file_s_checks(path: Path) -> None:
    with_users(path)

    with darudb.Database.open(path) as db:
        with db.read() as txn:
            alice = txn.collection("users").get(1)
            names = [user["name"] for user in txn.collection("users").find(F.age >= 0)]
            adults = txn.collection("users").count("age >= $0", 18)

        assert alice is not None
        assert alice["address"] == {"city": "Seoul", "postcode": "04524"}
        assert alice["tags"] == ["red"]
        assert names == ["Alice", "Bob"]
        assert adults == 1

        with db.write() as txn:
            users = txn.collection("users")

            # A float field takes an int, and an embedded object a dict, as
            # through a class.
            assert users.insert({"name": "Carol", "rating": 2, "address": {"city": "Busan"}}) == 3
            assert users.update(2, age=18)

            with pytest.raises(DaruError) as duplicate:
                users.insert({"name": "Eve", "email": "alice@example.com"})

            with pytest.raises(DaruError) as missing:
                users.insert({"age": 3})

            with pytest.raises(DaruError) as not_a_dict:
                users.insert(User(name="Dave"))

            assert users.find_one("name == $0", "Carol") == {
                "id": 3,
                "name": "Carol",
                "email": None,
                "age": 0,
                "rating": 2.0,
                "active": True,
                "tags": [],
                "avatar": None,
                "address": {"city": "Busan", "postcode": None},
            }

        assert duplicate.value.code == "DUPLICATE_KEY"
        assert missing.value.code == "INVALID_ARGUMENT"
        assert not_a_dict.value.code == "INVALID_ARGUMENT"

    # A database that declares the schema reads what was written without it.
    with open_db(path) as db, db.read() as txn:
        carol = txn.collection(User).get(3)

    assert carol is not None
    assert carol.rating == 2.0
    assert carol.address == Address(city="Busan")


def test_a_prepared_query_runs_on_a_collection_reached_by_name(path: Path) -> None:
    with_users(path)

    with darudb.Database.open(path) as db:
        older = db.prepare("users", "age >= $0")

        with db.read() as txn:
            assert [user["name"] for user in txn.collection("users").find(older, 18)] == ["Alice"]


def test_a_file_without_a_schema_has_no_collections_with_the_engine_s_reason(
    path: Path,
) -> None:
    with darudb.Database.open(path) as db:
        assert db.stored_schema is None
        assert db.schema_version is None

        with db.read() as txn, pytest.raises(DaruError) as none:
            txn.collection("users")

        with pytest.raises(DaruError) as prepared:
            db.prepare("users", "age >= 0")

        assert none.value.code == "INVALID_ARGUMENT"
        assert "holds none" in str(none.value)
        assert prepared.value.code == "INVALID_ARGUMENT"

        # A schema stored after the database opened is not its own.
        open_db(path).close()

        with db.read() as txn, pytest.raises(DaruError) as later:
            txn.collection("users")

        assert later.value.code == "INVALID_ARGUMENT"
        assert "open it again" in str(later.value)


def test_a_migration_under_it_is_noticed(path: Path) -> None:
    with_users(path)

    @darudb.collection("users")
    class UserV2:
        id: int | None = None
        name: str
        email: str | None = darudb.field(default=None, unique=True)
        age: int = darudb.field(default=0, index=True)
        rating: float = 0.0
        active: bool = True
        tags: list[str] = darudb.field(default_factory=list, index=True)
        avatar: bytes | None = None
        address: Address | None = None
        nick: str | None = None

    with darudb.Database.open(path) as db:
        darudb.Database.open(
            path, schema=darudb.Schema(2, [UserV2, *SCHEMA.collections[1:]])
        ).close()

        with db.read() as txn, pytest.raises(DaruError) as error:
            txn.collection("users").count()

        assert error.value.code == "SCHEMA_MISMATCH"
        assert db.schema_version == 1


def test_the_asynchronous_api_reaches_the_collections_by_name_too(path: Path) -> None:
    with_users(path)

    async def run() -> list[str]:
        async with await darudb.Database.open_async(path) as db:
            async with db.write_async() as txn:
                await txn.collection("users").insert({"name": "Dave"})

            async with db.read_async() as txn:
                found = await txn.collection("users").find()

        return [user["name"] for user in found]

    assert asyncio.run(run()) == ["Alice", "Bob", "Dave"]
