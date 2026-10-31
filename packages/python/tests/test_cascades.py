"""Cascading links: what deleting an object takes with it."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, F, Migration, field


@darudb.collection("authors")
class Author:
    id: int | None = None
    name: str


@darudb.collection("books")
class Book:
    id: int | None = None
    title: str
    author: int = field(link=Author, cascade=True, index=True)


@darudb.collection("reviews")
class Review:
    id: int | None = None
    book: int | None = field(default=None, link=Book, cascade=True)


@darudb.collection("notes")
class Note:
    id: int | None = None
    book: int = field(link=Book)


LIBRARY = darudb.Schema(1, [Author, Book, Review, Note])


@pytest.fixture
def db(path: Path) -> darudb.Database:
    database = darudb.Database.open(path, schema=LIBRARY)

    with database.write() as txn:
        for name in ["Ada", "Bo"]:
            author = txn.collection(Author).insert(Author(name=name))
            book = txn.collection(Book).insert(Book(title=name, author=author))

            txn.collection(Review).insert(Review(book=book))
            txn.collection(Note).insert(Note(book=book))

    yield database  # type: ignore[misc]
    database.close()


def counts(db: darudb.Database) -> list[int]:
    with db.read() as txn:
        return [txn.collection(each).count() for each in (Author, Book, Review, Note)]


def test_a_delete_goes_down_the_chain_and_stops_at_a_plain_link(db: darudb.Database) -> None:
    with db.write() as txn:
        assert txn.collection(Author).delete(1)
        assert not txn.collection(Author).delete(1)

    assert counts(db) == [1, 1, 1, 2]
    assert db.check().ok


def test_a_delete_by_query_counts_what_it_found(db: darudb.Database) -> None:
    with db.write() as txn:
        assert txn.collection(Author).delete_where(F.name == "Ada") == 1

    assert counts(db) == [1, 1, 1, 2]


def test_the_asynchronous_api_cascades(path: Path) -> None:
    async def main() -> None:
        db = await darudb.Database.open_async(path, schema=LIBRARY)

        async with db.write_async() as txn:
            author = await txn.collection(Author).insert(Author(name="Ada"))
            await txn.collection(Book).insert(Book(title="T", author=author))

        async with db.write_async() as txn:
            assert await txn.collection(Author).delete(author)

        async with db.read_async() as txn:
            assert await txn.collection(Book).count() == 0

        await db.close_async()

    asyncio.run(main())


def test_a_cycle_ends(path: Path) -> None:
    @darudb.collection("nodes")
    class Node:
        id: int | None = None
        next: int | None = field(default=None, link="nodes", cascade=True, index=True)

    with darudb.Database.open(path, schema=darudb.Schema(1, [Node])) as db, db.write() as txn:
        nodes = txn.collection(Node)

        nodes.insert(Node(next=2))
        nodes.insert(Node(next=1))
        nodes.insert(Node(next=3))
        nodes.insert(Node())
        nodes.delete(1)
        nodes.delete(3)

        assert nodes.count() == 1


def test_the_stored_schema_reads_a_cascade_back(db: darudb.Database) -> None:
    schema = db.stored_schema

    assert schema is not None

    fields = {
        (collection.name, each.name): each.cascade
        for collection in schema.collections
        for each in collection.fields
    }

    assert fields[("books", "author")]
    assert fields[("reviews", "book")]
    assert not fields[("notes", "book")]
    assert not fields[("books", "title")]


def test_a_migration_adds_a_cascade_and_deletes_through_it(path: Path) -> None:
    @darudb.collection("posts")
    class PlainPost:
        id: int | None = None
        author: int = field(link=Author)

    @darudb.collection("posts")
    class Post:
        id: int | None = None
        author: int = field(link=Author, cascade=True)

    with darudb.Database.open(path, schema=darudb.Schema(1, [Author, PlainPost])) as db:
        with db.write() as txn:
            txn.collection(Author).insert_many([Author(name="Ada"), Author(name="Bo")])
            txn.collection(PlainPost).insert_many([PlainPost(author=1), PlainPost(author=2)])

    def run(migrating: darudb.Migrating) -> None:
        migrating.collection(Author).delete(1)

    newer = darudb.Schema(2, [Author, Post])

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=newer, upgrade_format=False)

    assert error.value.code == "INVALID_ARGUMENT"

    with (
        darudb.Database.open(path, schema=newer, migrations=[Migration(2, run=run)]) as db,
        db.read() as txn,
    ):
        assert [post.author for post in txn.collection(Post).find()] == [2]


def test_a_cascade_is_refused_on_anything_but_a_link_to_one_object() -> None:
    with pytest.raises(DaruError):

        @darudb.collection("a")
        class NoLink:
            id: int | None = None
            name: str = field(cascade=True)

        darudb.Schema(1, [NoLink])

    with pytest.raises(DaruError):

        @darudb.collection("b")
        class ListOfLinks:
            id: int | None = None
            links: list[int] = field(default_factory=list, link=Author, cascade=True)

        darudb.Schema(1, [Author, ListOfLinks])

    with pytest.raises(DaruError):

        @darudb.embedded
        class Inside:
            author: int = field(link=Author, cascade=True)

        @darudb.collection("c")
        class Holding:
            id: int | None = None
            inside: Inside

        darudb.Schema(1, [Author, Holding])
