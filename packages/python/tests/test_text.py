"""Text indexes, and searching text for words."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F, param


@darudb.embedded
class Meta:
    note: str | None = darudb.field(default=None, name="memo")


@darudb.collection("notes", indexes=[darudb.index(F.meta.note, text=True)])
class Note:
    id: int | None = None
    title: str = darudb.field(index=True, text_index=True)
    body: str | None = darudb.field(default=None, text_index=True, name="text")
    tags: list[str] = darudb.field(default_factory=list, text_index=True)
    motto: str | None = None
    meta: Meta | None = None
    count: int = 0


SCHEMA = darudb.Schema(1, [Note])

NOTES = [
    Note(
        title="Quick Start",
        body="The QUICK brown fox.",
        tags=["red fox", "blue bird"],
        motto="Quick and quiet",
        meta=Meta(note="checked twice"),
    ),
    Note(title="Queries", body="A slow fox, quickly.", tags=["red bird"]),
    Note(title="검색", body="빠른 검색을 한다"),
    Note(title="東京", body="東京都に住む", motto="quick"),
    Note(title="Nothing"),
]


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = darudb.Database.open(path, schema=SCHEMA)

    with database.write() as txn:
        txn.collection(Note).insert_many(NOTES)

    yield database  # type: ignore[misc]
    database.close()


def ids(notes: list[Note]) -> list[int | None]:
    return [note.id for note in notes]


def test_a_search_finds_the_texts_with_every_word(db: darudb.Database) -> None:
    with db.read() as txn:
        notes = txn.collection(Note)

        def found(condition: object) -> list[int | None]:
            assert notes.count(condition) == len(notes.find(condition))  # type: ignore[arg-type]

            return ids(notes.find(condition))  # type: ignore[arg-type]

        assert found(F.body.matches("fox")) == [1, 2]
        assert found(F.body.matches("FOX quick")) == [1]
        assert found(F.body.matches("quick* fox")) == [1, 2]
        assert found(F.body.matches("qui")) == []
        assert found(F.body.matches("")) == []
        assert found(~F.body.matches("fox")) == [3, 4, 5]
        # Ignoring case leaves a search as it is: it ignores case already.
        assert found(F.body.matches("Fox").ignore_case()) == [1, 2]
        # Hangul by the words between spaces, Han and kana by pairs.
        assert found(F.body.matches("검색")) == []
        assert found(F.body.matches("검색*")) == [3]
        assert found(F.body.matches("東京")) == [4]
        assert found(F.body.matches("京都に")) == [4]
        assert found(F.body.matches("大阪")) == []
        # A list: one string has every word.
        assert found(F.tags.matches("red")) == [1, 2]
        assert found(F.tags.matches("red bird")) == [2]
        # Through an embedded object, beside a plain index, and with others.
        assert found(F.meta.note.matches("twice")) == [1]
        assert found(F.title.matches("quick")) == [1]
        assert found(F.title == "Queries") == [2]
        assert found(F.body.matches("fox") & (F.id > 1)) == [2]
        assert found(F.body.matches("東京") | F.tags.matches("red bird")) == [2, 4]
        # A field without a text index is read and searched.
        assert found(F.motto.matches("quick")) == [1, 4]
        assert found(F.motto.matches("qui* and")) == [1]


def test_the_query_language_and_parameters_search_too(db: darudb.Database) -> None:
    with db.read() as txn:
        notes = txn.collection(Note)

        assert ids(notes.find('text MATCHES "quick fox"')) == [1]
        assert ids(notes.find("text MATCHES $0", "slow FOX")) == [2]
        assert notes.count("tags MATCHES $0 AND id > $1", "red", 1) == 1

    prepared = db.prepare(Note, F.body.matches(param(0)))

    with db.read() as txn:
        notes = txn.collection(Note)

        assert ids(notes.find(prepared, "fox")) == [1, 2]
        assert ids(notes.find(prepared, "검색*")) == [3]


def test_the_asynchronous_api_searches(path: Path) -> None:
    async def main() -> None:
        db = await darudb.Database.open_async(path, schema=SCHEMA)

        async with db.write_async() as txn:
            await txn.collection(Note).insert_many(NOTES)

        async with db.read_async() as txn:
            notes = txn.collection(Note)

            assert ids(await notes.find(F.body.matches("quick* fox"))) == [1, 2]
            assert ids(await notes.find('text MATCHES "東京"')) == [4]
            assert await notes.count(F.tags.matches("red bird")) == 1

        await db.close_async()

    asyncio.run(main())


def test_a_change_of_the_text_changes_what_a_search_finds(db: darudb.Database) -> None:
    with db.write() as txn:
        notes = txn.collection(Note)

        assert notes.update(1, body="A cat")
        assert notes.delete(2)
        assert ids(notes.find(F.body.matches("fox"))) == []
        assert ids(notes.find(F.body.matches("cat"))) == [1]

    assert db.check().ok


def test_the_stored_schema_says_which_indexes_hold_words(db: darudb.Database) -> None:
    stored = db.stored_schema

    assert stored is not None
    assert [
        (index.fields, index.unique, index.text) for index in stored.collections[0].indexes
    ] == [
        (("title",), False, False),
        (("title",), False, True),
        (("text",), False, True),
        (("tags",), False, True),
        (("meta.memo",), False, True),
    ]


def test_a_search_of_a_field_that_holds_no_text_is_refused(db: darudb.Database) -> None:
    with db.read() as txn:
        notes = txn.collection(Note)

        for condition in (F.count.matches("1"), F.body.matches(1), 'count MATCHES "1"'):
            with pytest.raises(DaruError) as error:
                notes.find(condition)  # type: ignore[arg-type]

            assert error.value.code == "INVALID_QUERY"


def test_a_text_index_is_declared_on_text_alone(path: Path) -> None:
    for make in (
        lambda: darudb.index("a", "b", text=True),
        lambda: darudb.index("a", text=True, unique=True),
        lambda: darudb.index("a", text=1),  # type: ignore[arg-type]
        lambda: darudb.field(text_index=1),  # type: ignore[call-overload]
    ):
        with pytest.raises(DaruError) as error:
            make()

        assert error.value.code == "INVALID_ARGUMENT"

    @darudb.collection("counts")
    class Count:
        id: int | None = None
        count: int = darudb.field(default=0, text_index=True)

    @darudb.embedded
    class Inner:
        note: str | None = darudb.field(default=None, text_index=True)

    @darudb.collection("outers")
    class Outer:
        id: int | None = None
        inner: Inner | None = None

    @darudb.collection("paths", indexes=[darudb.index("count", text=True)])
    class Pathed:
        id: int | None = None
        count: int = 0

    for cls in (Count, Outer, Pathed):
        with pytest.raises(DaruError) as error:
            darudb.Database.open(path, schema=darudb.Schema(1, [cls])).close()

        assert error.value.code == "INVALID_ARGUMENT"


def test_a_file_kept_from_upgrading_refuses_a_text_index(path: Path) -> None:
    @darudb.collection("notes")
    class Plain:
        id: int | None = None
        title: str

    darudb.Database.open(path, schema=darudb.Schema(1, [Plain]), upgrade_format=False).close()

    @darudb.collection("notes")
    class Searched:
        id: int | None = None
        title: str = darudb.field(text_index=True)

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=darudb.Schema(2, [Searched]), upgrade_format=False)

    assert error.value.code == "INVALID_ARGUMENT"

    with darudb.Database.open(path, schema=darudb.Schema(2, [Searched])) as db:
        with db.write() as txn:
            txn.collection(Searched).insert(Searched(title="Quick start"))

        with db.read() as txn:
            assert txn.collection(Searched).count(F.title.matches("start")) == 1
