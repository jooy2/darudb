"""The complex suite on DaruDB through its Python package, as an application
declares it: each collection a class, a link from each person to an
organization and from each post to its author, an embedded address, and lists
of tags with an index on each. Queries that run many times are prepared once,
and an update sets the one field it changes."""

from __future__ import annotations

from pathlib import Path

import complex as cx
import darudb
from common import Digest, Rows
from darudb import Query, field


@darudb.collection("organizations")
class Organization:
    code: str = field(primary_key=True)
    name: str
    kind: str = field(index=True)
    country: str = field(index=True)
    founded: int


@darudb.embedded
class Address:
    city: str
    zip: str


@darudb.collection("people")
class Person:
    id: int | None = None
    name: str
    email: str = field(unique=True)
    age: int = field(index=True)
    country: str = field(index=True)
    address: Address
    score: float
    active: bool
    joined_at: int = field(index=True)
    organization: str = field(link=Organization, index=True)
    tags: list[str] = field(index=True)


@darudb.collection("posts")
class Post:
    id: int | None = None
    author: int = field(link=Person, index=True)
    title: str
    body: str
    likes: int = field(index=True)
    language: str = field(index=True)
    created_at: int = field(index=True)
    pinned: bool
    tags: list[str] = field(index=True)


SCHEMA = darudb.Schema(1, [Organization, Person, Post])


def person_of(values: dict[str, object]) -> Person:
    city = values.pop("city")
    zip_ = values.pop("zip")

    return Person(**values, address=Address(city=city, zip=zip_))  # type: ignore[arg-type]


def run(directory: Path, rows: Rows) -> None:
    db = darudb.Database.open(directory / "complex.darudb", schema=SCHEMA)

    # The objects are made before the load's time starts.
    organizations = [
        Organization(**cx.organization(n))  # type: ignore[arg-type]
        for n in range(cx.ORGANIZATIONS)
    ]
    people = [person_of(cx.person(n)) for n in range(cx.PEOPLE)]
    posts = [Post(**cx.post(m)) for m in range(cx.POSTS)]  # type: ignore[arg-type]

    def load(d: Digest) -> None:
        for cls, objects in ((Organization, organizations), (Person, people), (Post, posts)):
            for at in range(0, len(objects), cx.BATCH):
                with db.write() as txn:
                    txn.collection(cls).insert_many(objects[at : at + cx.BATCH])

    rows.all(*cx.LOAD, load)
    del organizations, people, posts

    newest = db.prepare(Post, "author == $0 SORT BY created_at DESC LIMIT 5")
    filtered = db.prepare(
        Person,
        "age BETWEEN $0 AND $1 AND country == $2 AND active == true SORT BY score DESC LIMIT 50",
    )
    either = db.prepare(Person, "country IN [$0, $1] OR age >= 78")
    prefix = db.prepare(Person, "name STARTSWITH $0")
    author_country = db.prepare(
        Post, "author.country == $0 AND likes >= 900 SORT BY created_at DESC LIMIT 20"
    )
    two_links = db.prepare(Post, "author.organization.kind == $0 AND language == $1")
    tag = db.prepare(Post, "tags CONTAINS $0 SORT BY likes DESC LIMIT 20")
    text = db.prepare(Post, "body CONTAINS $0")
    likes = db.prepare(Post, "likes >= 500 AND language == $0")
    in_country = db.prepare(Person, "country == $0")
    by_author = db.prepare(Post, "author == $0")
    runs = cx.Runs

    with db.read() as txn:
        organizations_ = txn.collection(Organization)
        people_ = txn.collection(Person)
        posts_ = txn.collection(Post)

        def detail(r: int, d: Digest) -> None:
            found = people_.get(runs.detail(r))

            if found is None:
                return

            d.add(found.id, found.age)  # type: ignore[arg-type]
            organization = organizations_.get(found.organization)

            if organization is not None:
                d.add(organization.founded, 0)

            for p in posts_.find(newest, found.id):
                d.add(p.id, p.likes)  # type: ignore[arg-type]

        def people_filter(r: int, d: Digest) -> None:
            age, country = runs.filter(r)

            for p in people_.find(filtered, age, age + 9, country):
                d.add(p.id, p.age)  # type: ignore[arg-type]

        def people_prefix(r: int, d: Digest) -> None:
            for p in people_.find(prefix, runs.prefix(r)):
                d.add(p.id, p.age)  # type: ignore[arg-type]

        def people_page(r: int, d: Digest) -> None:
            page = Query().sort_by("joined_at").offset(runs.offset(r)).limit(50)

            for p in people_.find(page):
                d.add(p.id, p.age)  # type: ignore[arg-type]

        def posts_author_country(r: int, d: Digest) -> None:
            for p in posts_.find(author_country, runs.author_country(r)):
                d.add(p.id, p.likes)  # type: ignore[arg-type]

        def posts_tag(r: int, d: Digest) -> None:
            for p in posts_.find(tag, runs.tag(r)):
                d.add(p.id, p.likes)  # type: ignore[arg-type]

        rows.each(*cx.DETAIL, detail)
        rows.each(*cx.FILTER, people_filter)
        rows.each(*cx.EITHER, lambda r, d: d.number(people_.count(either, *runs.either(r))))
        rows.each(*cx.PREFIX, people_prefix)
        rows.each(*cx.PAGE, people_page)
        rows.each(*cx.AUTHOR_COUNTRY, posts_author_country)
        rows.each(
            *cx.TWO_LINKS, lambda r, d: d.number(posts_.count(two_links, *runs.two_links(r)))
        )
        rows.each(*cx.TAG, posts_tag)
        rows.each(*cx.TEXT, lambda r, d: d.number(posts_.count(text, runs.phrase(r))))
        rows.each(*cx.COUNT, lambda r, d: d.number(posts_.count(likes, runs.language(r))))

    def update(d: Digest) -> None:
        with db.write() as txn:
            people = txn.collection(Person)

            for country in runs.UPDATED:
                found = people.find(in_country, country)
                d.number(len(found))

                for p in found:
                    people.update(p.id, age=(p.age + 1) % 80)  # type: ignore[arg-type]

    def delete(d: Digest) -> None:
        with db.write() as txn:
            people = txn.collection(Person)
            posts = txn.collection(Post)

            for r in range(runs.DELETED):
                id_ = runs.deleted(r)
                deleted = 0

                for p in posts.find(by_author, id_):
                    if posts.delete(p.id):  # type: ignore[arg-type]
                        deleted += 1

                if people.delete(id_):
                    d.add(id_, deleted)

    rows.all(cx.UPDATE, cx.updated_count(), update)
    rows.all(cx.DELETE, runs.DELETED, delete)

    left_people = Digest()
    left_posts = Digest()

    with db.read() as txn:
        for p in txn.collection(Person).find():
            left_people.add(p.id, p.age)  # type: ignore[arg-type]

        for p in txn.collection(Post).find():
            left_posts.add(p.id, p.likes)  # type: ignore[arg-type]

    rows.check("left-people", left_people)
    rows.check("left-posts", left_posts)
    db.close()
