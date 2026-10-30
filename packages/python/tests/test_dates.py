"""Dates: aware ``datetime`` values, kept to the microsecond and read in UTC."""

from __future__ import annotations

from datetime import UTC, datetime, timedelta, timezone
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F

SEOUL = timezone(timedelta(hours=9))
EPOCH = datetime(1970, 1, 1, tzinfo=UTC)


@darudb.collection("events")
class Event:
    id: int | None = None
    name: str
    at: datetime = darudb.field(index=True)
    ends: datetime | None = None
    seen: list[datetime] = darudb.field(default_factory=list)
    due: datetime = EPOCH


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = darudb.Database.open(path, schema=darudb.Schema(1, [Event]))
    yield database  # type: ignore[misc]
    database.close()


LAUNCH = datetime(2026, 10, 30, 21, 0, 0, 123456, tzinfo=SEOUL)
BEFORE = datetime(1969, 12, 31, 23, 59, 59, 999999, tzinfo=UTC)
ANCIENT = datetime(1, 1, 1, tzinfo=UTC)


def write(db: darudb.Database) -> None:
    with db.write() as txn:
        events = txn.collection(Event)
        events.insert(Event(name="launch", at=LAUNCH, seen=[LAUNCH, BEFORE]))
        events.insert(Event(name="before", at=BEFORE, ends=LAUNCH))
        events.insert(Event(name="ancient", at=ANCIENT))


def test_a_date_reads_back_as_the_same_instant_in_utc(db: darudb.Database) -> None:
    write(db)

    with db.read() as txn:
        events = txn.collection(Event)
        launch = events.get(1)
        before = events.get(2)

    assert launch is not None and before is not None
    assert launch.at == LAUNCH
    assert launch.at.tzinfo == UTC
    assert launch.at.hour == 12
    assert launch.seen == [LAUNCH, BEFORE]
    assert before.at == BEFORE
    assert before.ends == LAUNCH
    assert before.due == EPOCH


def test_dates_are_queried_in_time_order(db: darudb.Database) -> None:
    write(db)

    def names(found: list[Event]) -> list[str]:
        return [event.name for event in found]

    with db.read() as txn:
        events = txn.collection(Event)

        assert names(events.find(darudb.where(F.at < datetime(1970, 1, 1, tzinfo=UTC)))) == [
            "before",
            "ancient",
        ]
        assert names(events.find(darudb.Query().sort_by(F.at))) == ["ancient", "before", "launch"]
        assert names(events.find('at >= DATE "2026-10-30T12:00:00Z"')) == ["launch"]
        assert names(events.find("at < $0", BEFORE + timedelta(microseconds=1))) == [
            "before",
            "ancient",
        ]
        assert names(events.find(F.seen.contains(BEFORE))) == ["launch"]
        assert events.min(F.at) == ANCIENT
        assert events.max(F.ends) == LAUNCH

        with pytest.raises(DaruError) as error:
            events.find(F.at > 0)

        assert error.value.code == "INVALID_QUERY"

        with pytest.raises(DaruError) as error:
            events.sum(F.at)

        assert error.value.code == "INVALID_QUERY"


def test_a_naive_datetime_is_refused(db: darudb.Database) -> None:
    with db.write() as txn:
        events = txn.collection(Event)

        with pytest.raises(DaruError) as error:
            events.insert(Event(name="naive", at=datetime(2026, 10, 30)))

        assert error.value.code == "INVALID_ARGUMENT"
        assert "time zone" in error.value.message

        with pytest.raises(DaruError) as error:
            events.find(F.at < datetime(2026, 10, 30))

        assert error.value.code == "INVALID_QUERY"


def test_the_stored_schema_names_the_date_type(db: darudb.Database) -> None:
    schema = db.stored_schema

    assert schema is not None

    (events,) = schema.collections
    by_name = {field.name: field for field in events.fields}

    assert by_name["at"].type.kind == "date"
    assert by_name["seen"].type.element is not None
    assert by_name["seen"].type.element.kind == "date"
