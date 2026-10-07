"""Writing and reading objects, and what a transaction sees."""

from __future__ import annotations

import dataclasses
from pathlib import Path

import pytest

import darudb
from darudb import DaruError
from models import Address, Blob, Post, User, open_db


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = open_db(path)
    yield database  # type: ignore[misc]
    database.close()


def test_insert_gives_auto_increment_keys_and_get_reads_the_object(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        first = users.insert(User(name="Alice", age=31))
        second = users.insert(User(name="Bob"))

    assert (first, second) == (1, 2)

    with db.read() as txn:
        alice = txn.collection(User).get(1)

    assert alice == User(id=1, name="Alice", age=31)
    assert alice is not None and alice.id == 1
    assert txn is not None


def test_an_object_read_is_a_frozen_instance_of_its_class(db: darudb.Database) -> None:
    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))

    with db.read() as txn:
        alice = txn.collection(User).get(1)

    assert type(alice) is User
    assert alice is not None

    with pytest.raises(dataclasses.FrozenInstanceError):
        alice.name = "Eve"  # type: ignore[misc]

    assert dataclasses.replace(alice, name="Eve").name == "Eve"


def test_every_field_type_round_trips(db: darudb.Database) -> None:
    user = User(
        name="Alice",
        email="alice@example.com",
        age=-(2**63),
        rating=4.5,
        active=False,
        tags=["a", "b"],
        avatar=b"\x00\xff",
        address=Address(city="Seoul", zip="04524"),
    )

    with db.write() as txn:
        key = txn.collection(User).insert(user)

    with db.read() as txn:
        assert txn.collection(User).get(key) == dataclasses.replace(user, id=key)


def test_defaults_and_none_are_what_the_schema_says(db: darudb.Database) -> None:
    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))

    with db.read() as txn:
        alice = txn.collection(User).get(1)

    assert alice == User(id=1, name="Alice", age=0, rating=0.0, active=True, tags=[])
    assert alice is not None and alice.email is None and alice.address is None


def test_a_float_field_takes_an_int_and_an_int_field_refuses_a_float(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        users.insert(User(name="Alice", rating=4))

        with pytest.raises(DaruError) as error:
            users.insert(User(name="Bob", age=4.5))  # type: ignore[arg-type]

        assert error.value.code == "INVALID_ARGUMENT"

        with pytest.raises(DaruError) as error:
            users.insert(User(name="Bob", age=True))

        assert error.value.code == "INVALID_ARGUMENT"

    with db.read() as txn:
        alice = txn.collection(User).get(1)

    assert alice is not None and alice.rating == 4.0 and isinstance(alice.rating, float)


def test_bytes_come_in_any_buffer_and_go_out_as_bytes(db: darudb.Database) -> None:
    with db.write() as txn:
        blobs = txn.collection(Blob)
        blobs.insert(Blob(digest=bytearray(b"ab"), size=1))  # type: ignore[arg-type]
        blobs.insert(Blob(digest=memoryview(b"cd"), size=2))  # type: ignore[arg-type]

    with db.read() as txn:
        blobs = txn.collection(Blob)

        assert blobs.get(b"ab") == Blob(digest=b"ab", size=1)
        assert blobs.get(memoryview(b"cd")) == Blob(digest=b"cd", size=2)  # type: ignore[arg-type]


def test_a_value_python_cannot_store_is_invalid(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)

        for user in (
            User(name=3),  # type: ignore[arg-type]
            User(name="A", age=2**63),
            User(name="A", tags="ab"),  # type: ignore[arg-type]
            User(name="A", tags=[None]),  # type: ignore[list-item]
            User(name="A", address={"city": "Seoul"}),  # type: ignore[arg-type]
            User(name=None),  # type: ignore[arg-type]
        ):
            with pytest.raises(DaruError) as error:
                users.insert(user)

            assert error.value.code == "INVALID_ARGUMENT", user

        with pytest.raises(DaruError) as error:
            users.insert(Post(slug="x", author=1))  # type: ignore[arg-type]

        assert error.value.code == "INVALID_ARGUMENT"
        assert "expected a User" in error.value.message

        with pytest.raises(DaruError) as error:
            users.get(1.5)  # type: ignore[arg-type]

        assert error.value.code == "INVALID_ARGUMENT"


def test_a_declared_primary_key_and_put(db: darudb.Database) -> None:
    with db.write() as txn:
        posts = txn.collection(Post)

        assert posts.insert(Post(slug="hello", author=1, title="Hi")) == "hello"

        with pytest.raises(DaruError) as error:
            posts.insert(Post(slug="hello", author=2))

        assert error.value.code == "DUPLICATE_KEY"
        assert posts.put(Post(slug="hello", author=2, title="Hello")) == "hello"
        assert posts.put_many([Post(slug="a", author=1), Post(slug="b", author=1)]) == ["a", "b"]

    with db.read() as txn:
        assert txn.collection(Post).get("hello") == Post(slug="hello", author=2, title="Hello")
        assert txn.collection(Post).count() == 3


def test_a_unique_index_refuses_a_second_value_and_the_transaction_goes_on(
    db: darudb.Database,
) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        users.insert(User(name="Alice", email="a@example.com"))

        with pytest.raises(DaruError) as error:
            users.insert(User(name="Eve", email="a@example.com"))

        assert error.value.code == "DUPLICATE_KEY"
        users.insert(User(name="Bob", email=None))
        users.insert(User(name="Carol", email=None))

    with db.read() as txn:
        assert txn.collection(User).count() == 3


def test_insert_many_stops_at_a_refused_object_and_keeps_the_ones_before(
    db: darudb.Database,
) -> None:
    with db.write() as txn:
        users = txn.collection(User)

        assert users.insert_many([User(name="A"), User(name="B")]) == [1, 2]

        with pytest.raises(DaruError) as error:
            users.insert_many(
                [User(name="C", email="x"), User(name="D", email="x"), User(name="E")]
            )

        assert error.value.code == "DUPLICATE_KEY"

        # An object that does not convert refuses the batch before anything is written.
        with pytest.raises(DaruError):
            users.insert_many([User(name="F"), "not a user"])  # type: ignore[list-item]

    with db.read() as txn:
        assert [user.name for user in txn.collection(User).find()] == ["A", "B", "C"]


def test_update_sets_the_fields_it_names(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        users.insert(User(name="Alice", age=31, email="a@example.com", tags=["x"]))

        assert users.update(1, age=32, email=None, tags=["y", "z"])
        assert users.update(1, rating=None)
        assert not users.update(99, age=1)

        with pytest.raises(DaruError) as error:
            users.update(1, nickname="Al")

        assert error.value.code == "INVALID_ARGUMENT"

        with pytest.raises(DaruError) as error:
            users.update(1, name=None)

        assert error.value.code == "INVALID_ARGUMENT"

    with db.read() as txn:
        assert txn.collection(User).get(1) == User(id=1, name="Alice", age=32, tags=["y", "z"])


def test_update_finds_a_field_by_its_attribute_and_stores_its_name(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        users.insert(User(name="Alice", address=Address(city="Seoul")))
        users.update(1, address=Address(city="Busan", zip="48058"))

    with db.read() as txn:
        assert txn.collection(User).find_one("address.postcode == $0", "48058") is not None


def test_delete_says_whether_there_was_an_object(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        users.insert(User(name="Alice"))

        assert users.delete(1)
        assert not users.delete(1)
        assert users.get(1) is None


def test_a_block_that_raises_aborts_the_write(db: darudb.Database) -> None:
    with pytest.raises(RuntimeError), db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))
        raise RuntimeError("stop")

    with db.read() as txn:
        assert txn.collection(User).count() == 0


def test_a_read_sees_one_commit_throughout(db: darudb.Database, path: Path) -> None:
    with db.read() as txn:
        with open_db(path) as other, other.write() as write:
            write.collection(User).insert(User(name="Alice"))

        assert txn.collection(User).count() == 0

    with db.read() as txn:
        assert txn.collection(User).count() == 1


def test_a_write_sees_its_own_changes(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        users.insert(User(name="Alice", age=31))

        assert users.count() == 1
        assert users.find_one(darudb.F.age == 31) is not None


def test_a_deferred_commit_is_seen_at_once_and_made_durable_by_sync(db: darudb.Database) -> None:
    with db.write(durability="deferred") as txn:
        txn.collection(User).insert(User(name="Alice"))

    with db.read() as txn:
        assert txn.collection(User).count() == 1

    db.sync()

    with pytest.raises(DaruError) as error:
        db.write(durability="later")  # type: ignore[arg-type]

    assert error.value.code == "INVALID_ARGUMENT"


def test_write_transactions_on_one_file_do_not_nest(db: darudb.Database, path: Path) -> None:
    other = open_db(path)

    with db.write():
        with pytest.raises(DaruError) as error, db.write():
            pass

        assert error.value.code == "INVALID_ARGUMENT"

        # Another handle to the file is the same file.
        with pytest.raises(DaruError) as error, other.write():
            pass

        assert error.value.code == "INVALID_ARGUMENT"

    other.close()

    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))


def test_a_transaction_used_after_its_block_is_closed(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)

    with pytest.raises(DaruError) as error:
        users.insert(User(name="Alice"))

    assert error.value.code == "CLOSED"

    with db.read() as read:
        found = read.collection(User)

    with pytest.raises(DaruError) as error:
        found.count()

    assert error.value.code == "CLOSED"


def test_a_collection_is_reached_by_its_class_or_its_name(db: darudb.Database) -> None:
    with db.write() as txn:
        txn.collection("users").insert(User(name="Alice"))

        assert txn.collection(User).name == "users"
        assert repr(txn.collection(User)) == "<WriteCollection 'users'>"

        with pytest.raises(DaruError) as error:
            txn.collection("nobody")

        assert error.value.code == "INVALID_ARGUMENT"


def test_a_link_holds_the_key_and_a_list_of_links_the_keys(db: darudb.Database) -> None:
    with db.write() as txn:
        users = txn.collection(User)
        alice, bob = users.insert_many([User(name="Alice"), User(name="Bob")])
        assert isinstance(alice, int) and isinstance(bob, int)
        txn.collection(Post).insert(Post(slug="hi", author=alice, readers=[alice, bob]))

        with pytest.raises(DaruError) as error:
            txn.collection(Post).insert(Post(slug="no", author="alice"))  # type: ignore[arg-type]

        assert error.value.code == "INVALID_ARGUMENT"

    with db.read() as txn:
        assert txn.collection(Post).get("hi") == Post(slug="hi", author=1, readers=[1, 2])


def test_a_batch_that_is_not_iterable_is_invalid(db: darudb.Database) -> None:
    with db.write() as txn, pytest.raises(DaruError) as error:
        txn.collection(User).insert_many(5)  # type: ignore[arg-type]

    assert error.value.code == "INVALID_ARGUMENT"


def test_calls_that_wait_for_the_writer_are_refused_inside_a_write(db: darudb.Database) -> None:
    with db.write():
        for refused in (
            db.sync,
            db.compact,
            db.close,
            lambda: db.set_key(bytes(32)),
            lambda: db.set_password("x"),
        ):
            with pytest.raises(DaruError) as error:
                refused()

            assert error.value.code == "INVALID_ARGUMENT"

    db.sync()
    assert db.is_open


def test_prepare_on_a_closed_database_is_closed(path: Path) -> None:
    db = open_db(path)
    db.close()

    with pytest.raises(DaruError) as error:
        db.prepare(User, "age > $0")

    assert error.value.code == "CLOSED"
