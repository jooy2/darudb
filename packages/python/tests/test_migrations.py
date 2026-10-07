"""Migrations from one schema version to the next."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, Migration, field


@darudb.collection("people")
class PersonV1:
    id: int | None = None
    full_name: str = field(name="fullName")
    age: str


def fill_v1(path: Path) -> None:
    with darudb.Database.open(path, schema=darudb.Schema(1, [PersonV1])) as db, db.write() as txn:
        txn.collection(PersonV1).insert_many(
            [PersonV1(full_name="Alice", age="31"), PersonV1(full_name="Bob", age="17")]
        )


@darudb.collection("users")
class UserV2:
    id: int | None = None
    name: str
    age: int = 0
    city: str = field(default="Seoul", index=True)


V2 = darudb.Schema(2, [UserV2])


def renames(run: object = None) -> Migration:
    return Migration(
        2,
        rename_collections=[("people", "users")],
        rename_fields=[("people", "fullName", "name")],
        replace_fields=[("people", "age")],
        run=run,  # type: ignore[arg-type]
    )


def test_a_migration_renames_replaces_and_runs_its_function(path: Path) -> None:
    fill_v1(path)
    seen: list[tuple[int, int]] = []

    def run(migrating: darudb.Migrating) -> None:
        seen.append((migrating.previous_version, migrating.version))
        users = migrating.collection(UserV2)

        for key in migrating.previous_keys("people"):
            old = migrating.previous("people", key)
            assert old is not None
            users.update(key, age=int(old["age"]))

        assert migrating.previous("people", 99) is None

    with darudb.Database.open(path, schema=V2, migrations=[renames(run)]) as db:
        assert db.schema_version == 2

        with db.read() as txn:
            users = txn.collection(UserV2).find()

    assert seen == [(1, 2)]
    assert users == [
        UserV2(id=1, name="Alice", age=31, city="Seoul"),
        UserV2(id=2, name="Bob", age=17, city="Seoul"),
    ]


def test_a_failing_function_leaves_the_file_as_it_was(path: Path) -> None:
    fill_v1(path)

    def run(migrating: darudb.Migrating) -> None:
        migrating.collection(UserV2).update(1, age=99)
        raise RuntimeError("stop")

    with pytest.raises(RuntimeError):
        darudb.Database.open(path, schema=V2, migrations=[renames(run)])

    with darudb.Database.open(path, schema=darudb.Schema(1, [PersonV1])) as db:
        with db.read() as txn:
            assert txn.collection(PersonV1).get(1) == PersonV1(id=1, full_name="Alice", age="31")


def test_a_coroutine_function_needs_open_async(path: Path) -> None:
    fill_v1(path)

    async def run(migrating: darudb.AsyncMigrating) -> None:
        await migrating.collection(UserV2).update(1, age=31)

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=V2, migrations=[renames(run)])

    assert error.value.code == "INVALID_ARGUMENT"
    assert "open_async" in error.value.message


def test_open_async_awaits_a_migration_function(path: Path) -> None:
    fill_v1(path)

    async def run(migrating: darudb.AsyncMigrating) -> None:
        users = migrating.collection(UserV2)

        for key in await migrating.previous_keys("people"):
            old = await migrating.previous("people", key)
            assert old is not None
            await users.update(key, age=int(old["age"]))

    async def main() -> list[UserV2]:
        db = await darudb.Database.open_async(path, schema=V2, migrations=[renames(run)])

        async with db, db.read_async() as txn:
            return await txn.collection(UserV2).find()

    assert [user.age for user in asyncio.run(main())] == [31, 17]


def test_an_async_migration_refuses_a_write_on_its_own_file(path: Path) -> None:
    fill_v1(path)

    async def run(migrating: darudb.AsyncMigrating) -> None:
        other = await darudb.Database.open_async(path)

        try:
            async with other.write_async():
                pass
        finally:
            other.close()

    async def main() -> None:
        await darudb.Database.open_async(path, schema=V2, migrations=[renames(run)])

    with pytest.raises(DaruError) as error:
        asyncio.run(main())

    assert error.value.code == "INVALID_ARGUMENT"


def test_open_async_runs_a_plain_function_too(path: Path) -> None:
    fill_v1(path)

    def run(migrating: darudb.AsyncMigrating) -> None:
        assert migrating.version == 2

    async def main() -> int | None:
        async with await darudb.Database.open_async(
            path, schema=V2, migrations=[renames(run)]
        ) as db:
            return db.schema_version

    assert asyncio.run(main()) == 2


def test_a_new_unique_index_that_finds_a_value_twice_fails_the_migration(path: Path) -> None:
    @darudb.collection("users")
    class First:
        id: int | None = None
        email: str

    @darudb.collection("users")
    class Second:
        id: int | None = None
        email: str = field(unique=True)

    with darudb.Database.open(path, schema=darudb.Schema(1, [First])) as db, db.write() as txn:
        txn.collection(First).insert_many([First(email="a"), First(email="a")])

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=darudb.Schema(2, [Second]))

    assert error.value.code == "DUPLICATE_KEY"


def test_a_deleted_collection_goes_with_its_objects(path: Path) -> None:
    @darudb.collection("logs")
    class Log:
        id: int | None = None
        line: str

    @darudb.collection("notes")
    class Note:
        id: int | None = None
        text: str = ""

    with darudb.Database.open(path, schema=darudb.Schema(1, [Log, Note])) as db:
        with db.write() as txn:
            txn.collection(Log).insert(Log(line="x"))

    with darudb.Database.open(
        path,
        schema=darudb.Schema(2, [Note]),
        migrations=[Migration(2, delete_collections=["logs"])],
    ) as db:
        assert db.check().ok


def test_migrations_are_checked(path: Path) -> None:
    for migrations in ([Migration(2), Migration(2)], ["not a migration"]):
        with pytest.raises(DaruError) as error:
            darudb.Database.open(path, schema=V2, migrations=migrations)  # type: ignore[arg-type]

        assert error.value.code == "INVALID_ARGUMENT"

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, migrations=[Migration(2)])

    assert error.value.code == "INVALID_ARGUMENT"


def test_each_step_knows_its_own_version(path: Path) -> None:
    @darudb.collection("notes")
    class First:
        id: int | None = None
        text: str

    @darudb.collection("notes")
    class Third:
        id: int | None = None
        text: str
        stars: int = 0

    with darudb.Database.open(path, schema=darudb.Schema(1, [First])) as db:
        with db.write() as txn:
            txn.collection(First).insert(First(text="hi"))

    seen: list[tuple[int, int]] = []

    def step(migrating: darudb.Migrating) -> None:
        seen.append((migrating.previous_version, migrating.version))

    with darudb.Database.open(
        path,
        schema=darudb.Schema(3, [Third]),
        migrations=[Migration(2, run=step), Migration(3, run=step)],
    ):
        pass

    assert seen == [(1, 2), (1, 3)]


def test_a_query_reused_with_another_class_of_its_collection_compiles_for_it(
    path: Path,
) -> None:
    @darudb.collection("people")
    class Before:
        id: int | None = None
        nick: str = field(name="nickname")

    @darudb.collection("people")
    class After:
        id: int | None = None
        nick: str

    query = darudb.where(darudb.F.nick == "al")

    with darudb.Database.open(path, schema=darudb.Schema(1, [Before])) as db:
        with db.write() as txn:
            txn.collection(Before).insert(Before(nick="al"))

        with db.read() as txn:
            assert len(txn.collection(Before).find(query)) == 1

    with darudb.Database.open(
        path,
        schema=darudb.Schema(2, [After]),
        migrations=[Migration(2, rename_fields=[("people", "nickname", "nick")])],
    ) as db:
        with db.read() as txn:
            assert len(txn.collection(After).find(query)) == 1
