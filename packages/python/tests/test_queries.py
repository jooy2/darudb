"""Queries: conditions, sorting, paging, the query language and prepared queries."""

from __future__ import annotations

from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F, Query, param, where
from models import Address, Post, User, open_db


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = open_db(path)

    with database.write() as txn:
        txn.collection(User).insert_many(
            [
                User(
                    name="Alice",
                    age=31,
                    rating=4.5,
                    tags=["admin", "staff"],
                    address=Address(city="Seoul", zip="04524"),
                ),
                User(name="Bob", age=17, email="bob@example.com", tags=["staff"]),
                User(name="Carol", age=45, rating=3.0, address=Address(city="Busan")),
                User(name="Dave", age=31, active=False),
            ]
        )
        txn.collection(Post).insert_many(
            [
                Post(slug="intro", author=1, readers=[2, 3], title="Hello"),
                Post(slug="news", author=3, title="News"),
            ]
        )

    yield database  # type: ignore[misc]
    database.close()


def names(users: list[User]) -> list[str]:
    return [user.name for user in users]


def test_comparisons(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)

        assert names(users.find(F.age == 31)) == ["Alice", "Dave"]
        assert names(users.find(F.age != 31)) == ["Bob", "Carol"]
        assert names(users.find(F.age < 31)) == ["Bob"]
        assert names(users.find(F.age <= 31)) == ["Alice", "Bob", "Dave"]
        assert names(users.find(F.age > 31)) == ["Carol"]
        assert names(users.find(F.age >= 31)) == ["Alice", "Carol", "Dave"]
        assert names(users.find(F.age.between(20, 40))) == ["Alice", "Dave"]
        assert names(users.find(F.name.is_in(["Bob", "Dave", "Zoe"]))) == ["Bob", "Dave"]
        assert names(users.find(F.active == False)) == ["Dave"]  # noqa: E712
        assert names(users.find(F.rating > 4)) == ["Alice"]


def test_strings_lists_and_nulls(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)

        assert names(users.find(F.name.contains("o"))) == ["Bob", "Carol"]
        assert names(users.find(F.name.startswith("Ca"))) == ["Carol"]
        assert names(users.find(F.name.endswith("e"))) == ["Alice", "Dave"]
        assert names(users.find(F.tags.contains("staff"))) == ["Alice", "Bob"]
        assert names(users.find(F.email == None)) == ["Alice", "Carol", "Dave"]  # noqa: E711
        assert names(users.find(F.email != None)) == ["Bob"]  # noqa: E711
        assert names(users.find(F.email.is_null())) == ["Alice", "Carol", "Dave"]
        assert names(users.find(F.address.city.is_not_null())) == ["Alice", "Carol"]


def test_conditions_combine(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)

        assert names(users.find((F.age >= 30) & (F.active == True))) == ["Alice", "Carol"]  # noqa: E712
        assert names(users.find((F.age < 20) | (F.age > 40))) == ["Bob", "Carol"]
        assert names(users.find(~(F.age == 31))) == ["Bob", "Carol"]
        assert names(users.find(where(F.age >= 30).where(F.name.startswith("D")))) == ["Dave"]


def test_a_condition_has_no_truth_value_and_combines_only_with_conditions() -> None:
    with pytest.raises(TypeError):
        bool(F.age == 1)

    with pytest.raises(TypeError):
        (F.age >= 30) & F.active  # type: ignore[operator]

    with pytest.raises(TypeError):
        (F.age > 1) and (F.age < 5)  # noqa: B018


def test_sort_offset_and_limit(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)
        by_age = Query().sort_by(F.age, descending=True).sort_by("name")

        assert names(users.find(by_age)) == ["Carol", "Alice", "Dave", "Bob"]
        assert names(users.find(by_age.offset(1).limit(2))) == ["Alice", "Dave"]
        assert users.count(by_age.limit(3)) == 3
        assert users.count(by_age.offset(3)) == 1
        assert users.find_one(by_age) == users.get(3)
        assert users.find_one(F.age > 100) is None


def test_a_query_is_immutable_and_reusable(db: darudb.Database) -> None:
    base = where(F.age >= 18)
    sorted_query = base.sort_by(F.name, descending=True)

    with db.read() as txn:
        users = txn.collection(User)

        assert names(users.find(base)) == ["Alice", "Carol", "Dave"]
        assert names(users.find(sorted_query)) == ["Dave", "Carol", "Alice"]
        assert names(users.find(base)) == ["Alice", "Carol", "Dave"]


def test_paths_through_embedded_objects_and_links(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)
        posts = txn.collection(Post)

        assert names(users.find(F.address.city == "Busan")) == ["Carol"]
        # `zip` is stored as `postcode`; the attribute finds it.
        assert names(users.find(F.address.zip == "04524")) == ["Alice"]
        assert names(users.find(F["address"]["zip"].startswith("045"))) == ["Alice"]
        assert [post.slug for post in posts.find(F.author.name == "Carol")] == ["news"]
        assert [post.slug for post in posts.find(F.readers.contains(2))] == ["intro"]
        assert [post.slug for post in posts.find(Query().sort_by("author.name"))] == [
            "intro",
            "news",
        ]


def test_the_query_language_with_parameters(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)

        assert names(users.find("age >= $0 SORT BY age DESC LIMIT 2", 18)) == ["Carol", "Alice"]
        assert names(users.find('name STARTSWITH "B"')) == ["Bob"]
        assert users.count("age == $0", 31) == 2
        assert users.find_one("email == $0", None) == users.get(1)
        assert users.find_one("address.postcode == $0", "04524") == users.get(1)


def test_prepared_queries_from_text_and_from_the_builder(db: darudb.Database) -> None:
    by_text = db.prepare(User, "age >= $0 SORT BY name")
    by_builder = db.prepare(User, where(F.age >= param(0)).sort_by(F.name))
    by_name = db.prepare(User, F.name == param(0))

    assert by_text.collection == "users"
    assert repr(param(2)) == "param(2)"

    with db.read() as txn:
        users = txn.collection(User)

        for prepared in (by_text, by_builder):
            assert names(users.find(prepared, 30)) == ["Alice", "Carol", "Dave"]
            assert names(users.find(prepared, 40)) == ["Carol"]
            assert users.count(prepared, 18) == 3

        assert users.find_one(by_name, "Bob") == users.get(2)

        with pytest.raises(DaruError) as error:
            users.find(by_name)

        assert error.value.code == "INVALID_QUERY"

        with pytest.raises(DaruError) as error:
            txn.collection(Post).find(by_name, "Bob")

        assert error.value.code == "INVALID_QUERY"


def test_a_query_the_engine_refuses_is_invalid_query(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)

        for query in (
            F.nickname == "x",
            F.age == "thirty",
            F.address == None,  # noqa: E711
            "age >>> 3",
        ):
            with pytest.raises(DaruError) as error:
                users.find(query)

            assert error.value.code == "INVALID_QUERY", query

        with pytest.raises(DaruError) as error:
            users.find(42)  # type: ignore[arg-type]

        assert error.value.code == "INVALID_QUERY"

        with pytest.raises(DaruError) as error:
            users.find(F.age == [1, 2])

        assert error.value.code == "INVALID_QUERY"

    with pytest.raises(DaruError) as error:
        F.age > None  # noqa: B015

    assert error.value.code == "INVALID_QUERY"

    with pytest.raises(DaruError):
        Query().limit(-1)

    with pytest.raises(DaruError):
        Query().where("age > 3")  # type: ignore[arg-type]


def test_a_filter_nests_at_most_24_deep(db: darudb.Database) -> None:
    condition = F.age == 1

    for _ in range(30):
        condition = ~condition

    with db.read() as txn, pytest.raises(DaruError) as error:
        txn.collection(User).find(condition)

    assert error.value.code == "INVALID_QUERY"


def test_a_query_with_a_float_compares_with_an_int_field(db: darudb.Database) -> None:
    with db.read() as txn:
        users = txn.collection(User)

        assert names(users.find(F.rating == 3)) == ["Carol"]
        assert names(users.find(F.rating >= 3.0)) == ["Alice", "Carol"]
