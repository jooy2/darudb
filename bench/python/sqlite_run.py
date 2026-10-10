"""SQLite through the standard library's ``sqlite3``. A write-ahead log, with
``synchronous = FULL`` for the sync commits and ``NORMAL`` for the deferred
ones; ``fullfsync`` is on, so that on Apple systems SQLite flushes the way
DaruDB does, and it changes nothing elsewhere. The page cache is 32 MiB,
DaruDB's default. Reads run in one transaction through statements the module
keeps prepared, and each row is made into an object."""

from __future__ import annotations

import sqlite3
from pathlib import Path

from common import OBJECTS, Digest, Person, Rows, person, random_ids, random_numbers

COLUMNS = "id, name, email, age, city, score"
INSERT = "INSERT INTO people (name, email, age, city, score) VALUES (:name, :email, :age, :city, :score)"


def open_db(path: Path) -> sqlite3.Connection:
    db = sqlite3.connect(path, isolation_level=None, cached_statements=64)

    db.executescript(
        """PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA fullfsync = ON;
        PRAGMA cache_size = -32768;
        CREATE TABLE IF NOT EXISTS people (
          id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
          age INTEGER NOT NULL, city TEXT NOT NULL, score REAL NOT NULL);
        CREATE INDEX IF NOT EXISTS people_age ON people (age DESC);"""
    )
    return db


def run(directory: Path, rows: Rows) -> None:
    db = open_db(directory / "commits.sqlite")

    def one(values: dict[str, object]) -> None:
        db.execute("BEGIN")
        db.execute(INSERT, values)
        db.execute("COMMIT")

    rows.each("insert-sync", 500, lambda r, d: one(person(r)))
    db.execute("PRAGMA synchronous = NORMAL")
    rows.each("insert-deferred", 10_000, lambda r, d: one(person(1_000 + r)))
    db.close()

    db = open_db(directory / "objects.sqlite")

    def bulk(d: Digest) -> None:
        db.execute("BEGIN")

        for n in range(OBJECTS):
            db.execute(INSERT, person(n))

        db.execute("COMMIT")

    rows.all("insert-bulk", OBJECTS, bulk)

    ids = random_ids(OBJECTS)
    emails = [f"{n}@example.com" for n in random_numbers(20_000)]
    by_id = f"SELECT {COLUMNS} FROM people WHERE id = ?"
    by_email = f"SELECT {COLUMNS} FROM people WHERE email = ?"
    by_age = f"SELECT {COLUMNS} FROM people WHERE age = ?"
    age_range = f"SELECT {COLUMNS} FROM people WHERE age BETWEEN ? AND ? ORDER BY age DESC, id ASC LIMIT 20"
    in_city = f"SELECT {COLUMNS} FROM people WHERE city = ?"
    top = f"SELECT {COLUMNS} FROM people ORDER BY score DESC, id ASC LIMIT 10"

    def each(sql: str, parameters: tuple[object, ...], d: Digest) -> None:
        for found in db.execute(sql, parameters):
            d.person(Person(*found))

    def get(sql: str, value: object, d: Digest) -> None:
        found = db.execute(sql, (value,)).fetchone()

        if found is not None:
            d.person(Person(*found))

    def count(r: int, d: Digest) -> None:
        d.number(
            db.execute("SELECT count(*) FROM people WHERE age >= ?", (40,)).fetchone()[
                0
            ]
        )

    db.execute("BEGIN")
    rows.each("get-key", OBJECTS, lambda r, d: get(by_id, ids[r], d))
    rows.each("get-email", 20_000, lambda r, d: get(by_email, emails[r], d))
    rows.each("age-equal", 200, lambda r, d: each(by_age, (r % 80,), d))
    rows.each("age-range", 5_000, lambda r, d: each(age_range, (r % 76, r % 76 + 4), d))
    rows.each("count", 200, count)
    rows.each("city-scan", 10, lambda r, d: each(in_city, (f"city {r % 100}",), d))
    rows.each("top-score", 10, lambda r, d: each(top, (), d))
    db.execute("COMMIT")

    def update(d: Digest) -> None:
        db.execute("BEGIN")

        for r in range(10_000):
            id_ = ids[r]
            found = db.execute(by_id, (id_,)).fetchone()

            if found is not None:
                p = Person(*found)
                p.age = (id_ + 1) % 80
                db.execute("UPDATE people SET age = ? WHERE id = ?", (p.age, id_))

        db.execute("COMMIT")

    def delete(d: Digest) -> None:
        db.execute("BEGIN")

        for r in range(10_000):
            if (
                db.execute("DELETE FROM people WHERE id = ?", (1 + r * 7,)).rowcount
                == 1
            ):
                d.number(1)

        db.execute("COMMIT")

    rows.all("update", 10_000, update)
    rows.all("delete", 10_000, delete)

    left = Digest()

    each(f"SELECT {COLUMNS} FROM people ORDER BY id", (), left)
    rows.check("left", left)
    db.close()
