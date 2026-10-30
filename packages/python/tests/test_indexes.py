"""Indexes on several fields, and on fields inside embedded objects."""

from __future__ import annotations

from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F


@darudb.embedded
class Home:
    city: str | None = darudb.field(default=None, name="town")


@darudb.collection(
    "people",
    indexes=[
        darudb.index("last", "first", unique=True),
        darudb.index(F.home.city, "age"),
    ],
)
class Person:
    id: int | None = None
    last: str
    first: str | None = darudb.field(default=None, name="given")
    age: int = 0
    home: Home | None = None


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = darudb.Database.open(path, schema=darudb.Schema(1, [Person]))
    yield database  # type: ignore[misc]
    database.close()


def test_an_index_on_several_fields_keeps_their_values_unique_together(
    db: darudb.Database,
) -> None:
    with db.write() as txn:
        people = txn.collection(Person)
        people.insert(Person(last="Kim", first="Ada", age=36, home=Home(city="Seoul")))
        people.insert(Person(last="Kim", first="Bo", age=20, home=Home(city="Seoul")))
        people.insert(Person(last="Kim", age=50))
        people.insert(Person(last="Kim", age=51))

        with pytest.raises(DaruError) as error:
            people.insert(Person(last="Kim", first="Ada"))

        assert error.value.code == "DUPLICATE_KEY"

    with db.read() as txn:
        people = txn.collection(Person)

        assert [
            person.first for person in people.find((F.home.city == "Seoul") & (F.age >= 30))
        ] == ["Ada"]
        assert [
            person.first
            for person in people.find(darudb.where(F.home.city == "Seoul").sort_by(F.age))
        ] == ["Bo", "Ada"]
        assert people.count((F.last == "Kim") & F.first.is_null()) == 2

    schema = db.stored_schema
    assert schema is not None
    assert [
        (index.field, index.fields, index.unique) for index in schema.collections[0].indexes
    ] == [
        ("last", ("last", "given"), True),
        ("home.town", ("home.town", "age"), False),
    ]
    assert db.check().ok


def test_an_index_names_its_fields_by_an_attribute_or_a_path() -> None:
    with pytest.raises(DaruError):
        darudb.index()

    with pytest.raises(DaruError):
        darudb.index("")

    with pytest.raises(DaruError):
        darudb.collection("x", indexes=["last"])  # type: ignore[list-item]


def test_an_index_on_a_field_the_class_lacks_fails_when_the_database_opens(path: Path) -> None:
    @darudb.collection("odd", indexes=[darudb.index("nope")])
    class Odd:
        id: int | None = None
        name: str

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=darudb.Schema(1, [Odd]))

    assert error.value.code == "INVALID_ARGUMENT"
