"""The asynchronous API: the same operations, awaited."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F, PasswordHashing, where
from models import SCHEMA, Post, User, open_db


def run(main: object) -> object:
    return asyncio.run(main)  # type: ignore[arg-type]


def test_open_read_and_write(path: Path) -> None:
    async def main() -> None:
        db = await darudb.Database.open_async(path, schema=SCHEMA)

        async with db.write_async() as txn:
            users = txn.collection(User)
            first = await users.insert(User(name="Alice", age=31))
            rest = await users.insert_many([User(name="Bob", age=17), User(name="Carol")])
            await users.put(User(id=first, name="Alice", age=32))
            assert await users.update(2, age=18)
            assert not await users.delete(99)
            await txn.collection(Post).put_many([Post(slug="a", author=first)])

        assert (first, rest) == (1, [2, 3])

        async with db.read_async() as txn:
            users = txn.collection(User)

            assert await users.get(1) == User(id=1, name="Alice", age=32)
            assert [user.name for user in await users.find(where(F.age >= 18))] == ["Alice", "Bob"]
            assert await users.find_one("age == $0", 18) == await users.get(2)
            assert await users.count() == 3
            assert repr(users) == "<AsyncReadCollection 'users'>"

        await db.close_async()
        assert not db.is_open

    run(main())


def test_a_block_that_raises_aborts_the_write(path: Path) -> None:
    async def main() -> int:
        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            with pytest.raises(RuntimeError):
                async with db.write_async() as txn:
                    await txn.collection(User).insert(User(name="Alice"))
                    raise RuntimeError("stop")

            async with db.read_async() as txn:
                return await txn.collection(User).count()

    assert run(main()) == 0


def test_operations_of_one_transaction_run_in_the_order_called(path: Path) -> None:
    async def main() -> list[str]:
        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            async with db.write_async() as txn:
                users = txn.collection(User)
                await asyncio.gather(*(users.insert(User(name=f"u{n}")) for n in range(50)))

            async with db.read_async() as txn:
                return [user.name for user in await txn.collection(User).find()]

    assert run(main()) == [f"u{n}" for n in range(50)]


def test_writes_of_this_process_queue_and_all_commit(path: Path) -> None:
    async def write(db: darudb.Database, n: int) -> None:
        async with db.write_async(durability="deferred") as txn:
            users = txn.collection(User)
            await users.insert(User(name=f"u{n}"))
            await asyncio.sleep(0)
            await users.insert(User(name=f"v{n}"))

    async def main() -> int:
        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            # More writers than the pool has threads.
            await asyncio.gather(*(write(db, n) for n in range(80)))
            await db.sync_async()

            async with db.read_async() as txn:
                return await txn.collection(User).count()

    assert run(main()) == 160


def test_a_synchronous_write_on_the_loop_is_refused_while_an_async_write_holds_the_file(
    path: Path,
) -> None:
    async def main() -> None:
        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            async with db.write_async():
                for refused in (db.write().__enter__, db.sync, db.close, db.compact):
                    with pytest.raises(DaruError) as error:
                        refused()

                    assert error.value.code == "INVALID_ARGUMENT"

            # Free again once the write is done.
            with db.write() as txn:
                txn.collection(User).insert(User(name="Alice"))

            db.sync()

    run(main())


def test_an_async_write_inside_a_synchronous_one_is_refused(path: Path) -> None:
    async def main() -> None:
        with open_db(path) as db, db.write():
            with pytest.raises(DaruError) as error:
                async with db.write_async():
                    pass

            assert error.value.code == "INVALID_ARGUMENT"

    run(main())


def test_the_tools_and_key_changes_have_async_twins(path: Path, tmp_path: Path) -> None:
    key = bytes(range(32))
    cheap = PasswordHashing(memory_kib=64, iterations=1, parallelism=1)

    async def main() -> None:
        db = await darudb.Database.open_async(path, schema=SCHEMA, key=key, password_hashing=cheap)

        async with db.write_async() as txn:
            await txn.collection(User).insert(User(name="Alice"))

        assert (await db.check_async()).ok
        assert (await db.backup_async(tmp_path / "copy.darudb")).bytes > 0
        assert (await db.compact_async()).bytes_after > 0
        await db.set_key_async(bytes(32))
        await db.set_password_async("a password")
        await db.close_async()

        report = await darudb.Database.salvage_async(
            path, tmp_path / "saved.darudb", password="a password"
        )

        assert report.whole

    run(main())


def test_an_async_transaction_used_after_its_block_is_closed(path: Path) -> None:
    async def main() -> None:
        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            async with db.read_async() as txn:
                users = txn.collection(User)

            with pytest.raises(DaruError) as error:
                await users.count()

            assert error.value.code == "CLOSED"

    run(main())


def test_errors_of_async_calls_are_daru_errors(path: Path) -> None:
    async def main() -> None:
        with pytest.raises(DaruError) as error:
            await darudb.Database.open_async(path, create=False)

        assert error.value.code == "NOT_FOUND"

        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            async with db.write_async() as txn:
                users = txn.collection(User)
                await users.insert(User(name="A", email="x"))

                with pytest.raises(DaruError) as error:
                    await users.insert(User(name="B", email="x"))

                assert error.value.code == "DUPLICATE_KEY"

                for batch in (users.insert_many, users.put_many):
                    with pytest.raises(DaruError) as error:
                        await batch(42)  # type: ignore[arg-type]

                    assert error.value.code == "INVALID_ARGUMENT"

    run(main())


def test_a_write_awaited_inside_a_write_on_the_file_is_refused(path: Path) -> None:
    async def main() -> None:
        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            async with db.write_async() as txn:
                await txn.collection(User).insert(User(name="Alice"))

                for nested in (
                    lambda: db.write_async().__aenter__(),
                    db.sync_async,
                    db.compact_async,
                    db.close_async,
                ):
                    with pytest.raises(DaruError) as error:
                        await nested()

                    assert error.value.code == "INVALID_ARGUMENT"

                # A task made inside the write inherits what it holds.
                async def inner() -> None:
                    async with db.write_async():
                        pass

                with pytest.raises(DaruError) as error:
                    await asyncio.gather(asyncio.create_task(inner()))

                assert error.value.code == "INVALID_ARGUMENT"

            # Once the write is over, the same calls take their turn.
            async with db.write_async() as txn:
                assert await txn.collection(User).count() == 1

            await db.sync_async()

    run(main())


def test_a_task_made_outside_a_write_waits_for_its_turn(path: Path) -> None:
    async def main() -> list[str]:
        order: list[str] = []

        async with await darudb.Database.open_async(path, schema=SCHEMA) as db:
            started = asyncio.Event()

            async def first() -> None:
                async with db.write_async() as txn:
                    started.set()
                    await asyncio.sleep(0.05)
                    await txn.collection(User).insert(User(name="first"))
                    order.append("first")

            async def second() -> None:
                await started.wait()

                async with db.write_async() as txn:
                    await txn.collection(User).insert(User(name="second"))
                    order.append("second")

            await asyncio.gather(first(), second())

        return order

    assert run(main()) == ["first", "second"]
