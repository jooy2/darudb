"""What a class may declare, and how its annotations become fields."""

from __future__ import annotations

import dataclasses
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, field


def refused(*classes: type) -> str:
    with pytest.raises(DaruError) as error:
        darudb.Schema(1, list(classes))

    assert error.value.code == "INVALID_ARGUMENT"

    return error.value.message


def test_a_decorated_class_is_a_frozen_keyword_only_dataclass() -> None:
    @darudb.collection
    class Note:
        id: int | None = None
        text: str

    assert dataclasses.is_dataclass(Note)

    note = Note(text="hi")

    with pytest.raises(dataclasses.FrozenInstanceError):
        note.text = "bye"  # type: ignore[misc]

    with pytest.raises(TypeError):
        Note("hi")  # type: ignore[misc]

    assert darudb.Schema(1, [Note]).resolved(Note).name == "Note"


def test_a_class_that_is_a_dataclass_already_is_kept_as_it_is(path: Path) -> None:
    @darudb.collection("notes")
    @dataclasses.dataclass(frozen=True)
    class Note:
        text: str
        id: int | None = None

    with darudb.Database.open(path, schema=darudb.Schema(1, [Note])) as db:
        with db.write() as txn:
            txn.collection(Note).insert(Note("hi"))

        with db.read() as txn:
            assert txn.collection(Note).get(1) == Note("hi", 1)


def test_a_class_with_slots_is_refused() -> None:
    @darudb.collection("notes")
    @dataclasses.dataclass(frozen=True, slots=True)
    class Note:
        text: str
        id: int | None = None

    assert "__slots__" in refused(Note)


def test_a_collection_without_a_key_needs_an_id() -> None:
    @darudb.collection
    class Note:
        text: str

    assert "id: int | None = None" in refused(Note)


def test_a_primary_key_is_required_unique_and_of_a_key_type() -> None:
    @darudb.collection
    class Optional:
        code: str | None = field(default=None, primary_key=True)

    @darudb.collection
    class Defaulted:
        code: str = field(default="x", primary_key=True)

    @darudb.collection
    class Floating:
        code: float = field(primary_key=True)

    @darudb.collection
    class Twice:
        a: str = field(primary_key=True)
        b: str = field(primary_key=True)

    for cls in (Optional, Defaulted, Floating, Twice):
        refused(cls)


def test_an_optional_field_has_no_default_but_none() -> None:
    @darudb.collection
    class Note:
        id: int | None = None
        text: str | None = "x"

    assert "None" in refused(Note)


def test_types_a_field_cannot_hold() -> None:
    @darudb.embedded
    class Part:
        name: str

    @darudb.collection
    class Odd:
        id: int | None = None
        when: complex

    @darudb.collection
    class Nested:
        id: int | None = None
        grid: list[list[int]]

    @darudb.collection
    class Parts:
        id: int | None = None
        parts: list[Part]

    @darudb.collection
    class Holes:
        id: int | None = None
        values: list[int | None]

    @darudb.collection
    class Plain:
        id: int | None = None
        thing: dict  # type: ignore[type-arg]

    for cls in (Odd, Nested, Parts, Holes, Plain):
        refused(cls)


def test_an_embedded_object_has_no_key_or_index() -> None:
    @darudb.embedded
    class Part:
        name: str = field(index=True)

    @darudb.collection
    class Whole:
        id: int | None = None
        part: Part

    refused(Whole)


def test_a_link_holds_a_key_and_names_a_collection(path: Path) -> None:
    @darudb.embedded
    class Part:
        name: str

    @darudb.collection
    class Floating:
        id: int | None = None
        to: float = field(link="users")

    @darudb.collection
    class ToPart:
        id: int | None = None
        to: int = field(link=Part)

    for cls in (Floating, ToPart):
        refused(cls)

    @darudb.collection
    class Orphan:
        id: int | None = None
        to: int = field(link="nobody")

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=darudb.Schema(1, [Orphan]))

    assert error.value.code == "INVALID_ARGUMENT"


def test_annotations_may_name_classes_declared_later(path: Path) -> None:
    @darudb.collection("trips")
    class Trip:
        id: int | None = None
        start: Place
        stops: list[str] = field(default_factory=list)

    @darudb.embedded
    class Place:
        city: str

    globals()["Place"] = Place

    try:
        with darudb.Database.open(path, schema=darudb.Schema(1, [Trip])) as db:
            with db.write() as txn:
                txn.collection(Trip).insert(Trip(start=Place(city="Seoul")))

            with db.read() as txn:
                trip = txn.collection(Trip).get(1)

        assert trip == Trip(id=1, start=Place(city="Seoul"))
    finally:
        del globals()["Place"]


def test_a_name_that_does_not_exist_is_refused() -> None:
    @darudb.collection
    class Note:
        id: int | None = None
        text: Missing  # type: ignore[name-defined]  # noqa: F821

    assert "does not exist" in refused(Note)


def test_a_decorator_decorates_classes_only() -> None:
    with pytest.raises(DaruError):
        darudb.embedded(len)  # type: ignore[type-var]


def test_a_schema_lists_its_classes() -> None:
    @darudb.collection("notes")
    class Note:
        id: int | None = None
        text: str = ""

    schema = darudb.Schema(3, [Note])

    assert schema.version == 3
    assert schema.collections == (Note,)
    assert repr(schema) == "Schema(3, [Note])"
