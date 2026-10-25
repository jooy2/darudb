"""The complex suite on SQLite through the standard library's ``sqlite3``, with
the settings of the simple suite: a write-ahead log with ``synchronous =
FULL``, ``fullfsync``, and a 32 MiB page cache. The schema is what an SQL
application would write: a foreign key for each link, the address's fields as
columns of their own, and a table for each list, with an index on every column
the other stores index. A row is read with its tags, through a subquery, and
made into an object, so that every store makes the same objects."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from pathlib import Path

import complex as cx
from common import Digest, Rows

PERSON = """SELECT id, name, email, age, country, city, zip, score, active, joined_at,
  organization, (SELECT group_concat(tag) FROM person_tags WHERE person = people.id)
  FROM people"""

POST = """SELECT id, author, title, body, likes, language, created_at, pinned,
  (SELECT group_concat(tag) FROM post_tags WHERE post = posts.id) FROM posts"""

SCHEMA = """PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA fullfsync = ON;
PRAGMA cache_size = -32768;
CREATE TABLE organizations (
  code TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
  country TEXT NOT NULL, founded INTEGER NOT NULL);
CREATE INDEX organizations_kind ON organizations (kind);
CREATE INDEX organizations_country ON organizations (country);
CREATE TABLE people (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
  age INTEGER NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL,
  zip TEXT NOT NULL, score REAL NOT NULL, active INTEGER NOT NULL,
  joined_at INTEGER NOT NULL,
  organization TEXT NOT NULL REFERENCES organizations (code));
CREATE INDEX people_age ON people (age);
CREATE INDEX people_country ON people (country);
CREATE INDEX people_joined_at ON people (joined_at);
CREATE INDEX people_organization ON people (organization);
CREATE TABLE person_tags (person INTEGER NOT NULL, tag TEXT NOT NULL);
CREATE INDEX person_tags_person ON person_tags (person);
CREATE INDEX person_tags_tag ON person_tags (tag, person);
CREATE TABLE posts (
  id INTEGER PRIMARY KEY, author INTEGER NOT NULL REFERENCES people (id),
  title TEXT NOT NULL, body TEXT NOT NULL, likes INTEGER NOT NULL,
  language TEXT NOT NULL, created_at INTEGER NOT NULL, pinned INTEGER NOT NULL);
CREATE INDEX posts_author ON posts (author);
CREATE INDEX posts_likes ON posts (likes);
CREATE INDEX posts_language ON posts (language);
CREATE INDEX posts_created_at ON posts (created_at);
CREATE TABLE post_tags (post INTEGER NOT NULL, tag TEXT NOT NULL);
CREATE INDEX post_tags_post ON post_tags (post);
CREATE INDEX post_tags_tag ON post_tags (tag, post);"""


@dataclass(slots=True)
class Organization:
    code: str
    name: str
    kind: str
    country: str
    founded: int


@dataclass(slots=True)
class Person:
    """A person as the other stores give one back, made by hand."""

    id: int
    name: str
    email: str
    age: int
    country: str
    city: str
    zip: str
    score: float
    active: bool
    joined_at: int
    organization: str
    tags: list[str]


@dataclass(slots=True)
class Post:
    id: int
    author: int
    title: str
    body: str
    likes: int
    language: str
    created_at: int
    pinned: bool
    tags: list[str]


def _tags(text: str | None) -> list[str]:
    return [] if text is None else text.split(",")


def person_of(row: tuple) -> Person:  # type: ignore[type-arg]
    return Person(*row[:8], bool(row[8]), row[9], row[10], _tags(row[11]))


def post_of(row: tuple) -> Post:  # type: ignore[type-arg]
    return Post(*row[:7], bool(row[7]), _tags(row[8]))


def run(directory: Path, rows: Rows) -> None:
    db = sqlite3.connect(
        directory / "complex.sqlite", isolation_level=None, cached_statements=64
    )
    db.executescript(SCHEMA)

    # The objects are made before the load's time starts.
    organizations = [cx.organization(n) for n in range(cx.ORGANIZATIONS)]
    people = [cx.person(n) for n in range(cx.PEOPLE)]
    posts = [cx.post(m) for m in range(cx.POSTS)]

    def load(d: Digest) -> None:
        for batch in cx.batches(organizations):
            db.execute("BEGIN")
            db.executemany(
                "INSERT INTO organizations (code, name, kind, country, founded) "
                "VALUES (:code, :name, :kind, :country, :founded)",
                batch,
            )
            db.execute("COMMIT")

        for batch in cx.batches(people):
            db.execute("BEGIN")

            for p in batch:
                id_ = db.execute(
                    "INSERT INTO people (name, email, age, country, city, zip, score, "
                    "active, joined_at, organization) VALUES (:name, :email, :age, "
                    ":country, :city, :zip, :score, :active, :joined_at, :organization)",
                    p,
                ).lastrowid
                db.executemany(
                    "INSERT INTO person_tags (person, tag) VALUES (?, ?)",
                    [(id_, t) for t in p["tags"]],  # type: ignore[attr-defined]
                )

            db.execute("COMMIT")

        for batch in cx.batches(posts):
            db.execute("BEGIN")

            for p in batch:
                id_ = db.execute(
                    "INSERT INTO posts (author, title, body, likes, language, created_at, "
                    "pinned) VALUES (:author, :title, :body, :likes, :language, "
                    ":created_at, :pinned)",
                    p,
                ).lastrowid
                db.executemany(
                    "INSERT INTO post_tags (post, tag) VALUES (?, ?)",
                    [(id_, t) for t in p["tags"]],  # type: ignore[attr-defined]
                )

            db.execute("COMMIT")

    rows.all(*cx.LOAD, load)
    del organizations, people, posts

    runs = cx.Runs

    def people_(sql: str, parameters: tuple, d: Digest) -> None:  # type: ignore[type-arg]
        for row in db.execute(sql, parameters):
            p = person_of(row)
            d.add(p.id, p.age)

    def posts_(sql: str, parameters: tuple, d: Digest) -> None:  # type: ignore[type-arg]
        for row in db.execute(sql, parameters):
            p = post_of(row)
            d.add(p.id, p.likes)

    def count(sql: str, parameters: tuple, d: Digest) -> None:  # type: ignore[type-arg]
        d.number(db.execute(sql, parameters).fetchone()[0])

    def detail(r: int, d: Digest) -> None:
        row = db.execute(f"{PERSON} WHERE id = ?", (runs.detail(r),)).fetchone()

        if row is None:
            return

        found = person_of(row)
        d.add(found.id, found.age)
        o = db.execute(
            "SELECT code, name, kind, country, founded FROM organizations WHERE code = ?",
            (found.organization,),
        ).fetchone()

        if o is not None:
            d.add(Organization(*o).founded, 0)

        posts_(
            f"{POST} WHERE author = ? ORDER BY created_at DESC, id LIMIT 5", (found.id,), d
        )

    def people_filter(r: int, d: Digest) -> None:
        age, country = runs.filter(r)
        people_(
            f"{PERSON} WHERE age BETWEEN ? AND ? AND country = ? AND active = 1 "
            "ORDER BY score DESC, id LIMIT 50",
            (age, age + 9, country),
            d,
        )

    db.execute("BEGIN")
    rows.each(*cx.DETAIL, detail)
    rows.each(*cx.FILTER, people_filter)
    rows.each(
        *cx.EITHER,
        lambda r, d: count(
            "SELECT count(*) FROM people WHERE country IN (?, ?) OR age >= 78",
            runs.either(r),
            d,
        ),
    )
    rows.each(
        *cx.PREFIX,
        lambda r, d: people_(
            f"{PERSON} WHERE substr(name, 1, length(?1)) = ?1 ORDER BY id",
            (runs.prefix(r),),
            d,
        ),
    )
    rows.each(
        *cx.PAGE,
        lambda r, d: people_(
            f"{PERSON} ORDER BY joined_at, id LIMIT 50 OFFSET ?", (runs.offset(r),), d
        ),
    )
    rows.each(
        *cx.AUTHOR_COUNTRY,
        lambda r, d: posts_(
            f"{POST} WHERE likes >= 900 AND author IN "
            "(SELECT id FROM people WHERE country = ?) ORDER BY created_at DESC, id LIMIT 20",
            (runs.author_country(r),),
            d,
        ),
    )
    rows.each(
        *cx.TWO_LINKS,
        lambda r, d: count(
            "SELECT count(*) FROM posts JOIN people ON people.id = posts.author "
            "JOIN organizations ON organizations.code = people.organization "
            "WHERE organizations.kind = ? AND posts.language = ?",
            runs.two_links(r),
            d,
        ),
    )
    rows.each(
        *cx.TAG,
        lambda r, d: posts_(
            f"{POST} WHERE id IN (SELECT post FROM post_tags WHERE tag = ?) "
            "ORDER BY likes DESC, id LIMIT 20",
            (runs.tag(r),),
            d,
        ),
    )
    rows.each(
        *cx.TEXT,
        lambda r, d: count(
            "SELECT count(*) FROM posts WHERE instr(body, ?) > 0", (runs.phrase(r),), d
        ),
    )
    rows.each(
        *cx.COUNT,
        lambda r, d: count(
            "SELECT count(*) FROM posts WHERE likes >= 500 AND language = ?",
            (runs.language(r),),
            d,
        ),
    )
    db.execute("COMMIT")

    def update(d: Digest) -> None:
        db.execute("BEGIN")

        for country in runs.UPDATED:
            d.number(
                db.execute(
                    "UPDATE people SET age = (age + 1) % 80 WHERE country = ?", (country,)
                ).rowcount
            )

        db.execute("COMMIT")

    def delete(d: Digest) -> None:
        db.execute("BEGIN")

        for r in range(runs.DELETED):
            id_ = runs.deleted(r)
            db.execute(
                "DELETE FROM post_tags WHERE post IN (SELECT id FROM posts WHERE author = ?)",
                (id_,),
            )
            deleted = db.execute("DELETE FROM posts WHERE author = ?", (id_,)).rowcount
            db.execute("DELETE FROM person_tags WHERE person = ?", (id_,))

            if db.execute("DELETE FROM people WHERE id = ?", (id_,)).rowcount == 1:
                d.add(id_, deleted)

        db.execute("COMMIT")

    rows.all(cx.UPDATE, cx.updated_count(), update)
    rows.all(cx.DELETE, runs.DELETED, delete)

    left_people = Digest()
    left_posts = Digest()

    people_(f"{PERSON} ORDER BY id", (), left_people)
    posts_(f"{POST} ORDER BY id", (), left_posts)
    rows.check("left-people", left_people)
    rows.check("left-posts", left_posts)
    db.close()
