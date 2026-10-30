"""Lists of embedded objects: a ``list`` of an ``@embedded`` class."""

from __future__ import annotations

from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F


@darudb.embedded
class Stop:
    city: str
    minutes: int | None = darudb.field(default=None, name="mins")


@darudb.collection("trips")
class Trip:
    id: int | None = None
    name: str
    stops: list[Stop] = darudb.field(default_factory=list)


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = darudb.Database.open(path, schema=darudb.Schema(1, [Trip]))
    yield database  # type: ignore[misc]
    database.close()


def test_a_list_of_embedded_objects_reads_back_as_instances(db: darudb.Database) -> None:
    coast = Trip(name="coast", stops=[Stop(city="Busan", minutes=30), Stop(city="Ulsan")])

    with db.write() as txn:
        trips = txn.collection(Trip)
        trips.insert(coast)
        trips.insert(Trip(name="none"))

    with db.read() as txn:
        trips = txn.collection(Trip)
        first = trips.get(1)
        second = trips.get(2)

        assert first is not None and second is not None
        assert first.stops == coast.stops
        assert all(isinstance(stop, Stop) for stop in first.stops)
        assert second.stops == []
        assert [trip.id for trip in trips.find(F.stops.city == "Ulsan")] == [1]
        assert [trip.id for trip in trips.find(F.stops.minutes > 10)] == [1]
        assert [trip.id for trip in trips.find("stops.mins IS NULL")] == [1]

        with pytest.raises(DaruError) as error:
            trips.find(darudb.Query().sort_by(F.stops.city))

        assert error.value.code == "INVALID_QUERY"


def test_an_element_of_another_type_is_refused(db: darudb.Database) -> None:
    with db.write() as txn:
        trips = txn.collection(Trip)

        with pytest.raises(DaruError) as error:
            trips.insert(Trip(name="x", stops=["Busan"]))  # type: ignore[list-item]

        assert error.value.code == "INVALID_ARGUMENT"


def test_a_list_of_lists_is_refused() -> None:
    @darudb.collection("grids")
    class Grid:
        id: int | None = None
        rows: list[list[int]]

    with pytest.raises(DaruError) as error:
        darudb.Schema(1, [Grid])

    assert error.value.code == "INVALID_ARGUMENT"
