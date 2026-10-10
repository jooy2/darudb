"""DaruDB through its Python package, as an application uses it: the
collection is a class, and queries that run many times are prepared once."""

from __future__ import annotations

import dataclasses
from pathlib import Path

import darudb
from common import OBJECTS, Digest, Rows, person, random_ids, random_numbers
from darudb import F, Query, field, param, where


@darudb.collection("people")
class Person:
    id: int | None = None
    name: str
    email: str = field(unique=True)
    age: int = field(index=True)
    city: str
    score: float


SCHEMA = darudb.Schema(1, [Person])


def run(directory: Path, rows: Rows) -> None:
    db = darudb.Database.open(directory / "commits.darudb", schema=SCHEMA)

    def insert(round_: int, durability: str, offset: int) -> None:
        with db.write(durability=durability) as txn:  # type: ignore[arg-type]
            txn.collection(Person).insert(Person(**person(offset + round_)))  # type: ignore[arg-type]

    rows.each("insert-sync", 500, lambda r, d: insert(r, "sync", 0))
    rows.each("insert-deferred", 10_000, lambda r, d: insert(r, "deferred", 1_000))
    db.close()

    db = darudb.Database.open(directory / "objects.darudb", schema=SCHEMA)

    def bulk(d: Digest) -> None:
        with db.write() as txn:
            people = txn.collection(Person)

            for n in range(OBJECTS):
                people.insert(Person(**person(n)))  # type: ignore[arg-type]

    rows.all("insert-bulk", OBJECTS, bulk)

    ids = random_ids(OBJECTS)
    emails = [f"{n}@example.com" for n in random_numbers(20_000)]
    by_email = db.prepare(Person, F.email == param(0))
    by_age = db.prepare(Person, F.age == param(0))
    age_range = db.prepare(
        Person,
        where(F.age.between(param(0), param(1)))
        .sort_by(F.age, descending=True)
        .limit(20),
    )
    at_least = F.age >= 40
    in_city = db.prepare(Person, F.city == param(0))
    top = Query().sort_by(F.score, descending=True).limit(10)

    with db.read() as txn:
        people = txn.collection(Person)

        def get_key(r: int, d: Digest) -> None:
            found = people.get(ids[r])

            if found is not None:
                d.person(found)

        def get_email(r: int, d: Digest) -> None:
            found = people.find_one(by_email, emails[r])

            if found is not None:
                d.person(found)

        def age_equal(r: int, d: Digest) -> None:
            for found in people.find(by_age, r % 80):
                d.person(found)

        def age_between(r: int, d: Digest) -> None:
            age = r % 76

            for found in people.find(age_range, age, age + 4):
                d.person(found)

        def city_scan(r: int, d: Digest) -> None:
            for found in people.find(in_city, f"city {r % 100}"):
                d.person(found)

        def top_score(r: int, d: Digest) -> None:
            for found in people.find(top):
                d.person(found)

        rows.each("get-key", OBJECTS, get_key)
        rows.each("get-email", 20_000, get_email)
        rows.each("age-equal", 200, age_equal)
        rows.each("age-range", 5_000, age_between)
        rows.each("count", 200, lambda r, d: d.number(people.count(at_least)))
        rows.each("city-scan", 10, city_scan)
        rows.each("top-score", 10, top_score)

    def update(d: Digest) -> None:
        with db.write() as txn:
            people = txn.collection(Person)

            for r in range(10_000):
                id_ = ids[r]
                found = people.get(id_)

                if found is not None:
                    people.put(dataclasses.replace(found, age=(id_ + 1) % 80))

    def delete(d: Digest) -> None:
        with db.write() as txn:
            people = txn.collection(Person)

            for r in range(10_000):
                if people.delete(1 + r * 7):
                    d.number(1)

    rows.all("update", 10_000, update)
    rows.all("delete", 10_000, delete)

    left = Digest()

    with db.read() as txn:
        for found in txn.collection(Person).find():
            left.person(found)

    rows.check("left", left)
    db.close()
