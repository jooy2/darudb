"""LMDB through py-lmdb. A sync commit is LMDB's default commit; a deferred
one runs in an environment opened with ``sync=False``, which is synced once
at the end, as closing a DaruDB file syncs its deferred commits. The record
is the fixed layout the Rust harness writes, and the unique and age indexes
are databases of their own, written by hand as an application would."""

from __future__ import annotations

import struct
from pathlib import Path

import lmdb
from common import OBJECTS, Digest, Person, Rows, person, random_ids, random_numbers

_ID = struct.Struct(">Q")
_AGE = struct.Struct(">QQ")
_HEAD = struct.Struct("<qd")
_LENGTH = struct.Struct("<H")


def encode(p: dict[str, object] | Person) -> bytes:
    get = p.__getitem__ if isinstance(p, dict) else lambda name: getattr(p, name)
    parts = [_HEAD.pack(get("age"), get("score"))]

    for name in ("name", "email", "city"):
        text = get(name).encode()
        parts.append(_LENGTH.pack(len(text)))
        parts.append(text)

    return b"".join(parts)


def decode(id_: int, data: bytes) -> Person:
    age, score = _HEAD.unpack_from(data)
    at = 16
    texts = []

    for _ in range(3):
        (length,) = _LENGTH.unpack_from(data, at)
        texts.append(data[at + 2 : at + 2 + length].decode())
        at += 2 + length

    return Person(id_, texts[0], texts[1], age, texts[2], score)


def city_of(data: bytes) -> bytes:
    at = 16

    for _ in range(2):
        at += 2 + _LENGTH.unpack_from(data, at)[0]

    (length,) = _LENGTH.unpack_from(data, at)
    return data[at + 2 : at + 2 + length]


class Store:
    def __init__(self, path: Path, sync: bool) -> None:
        self.env = lmdb.open(str(path), map_size=1 << 30, max_dbs=4, sync=sync)
        self.people = self.env.open_db(b"people")
        self.email = self.env.open_db(b"email")
        self.age = self.env.open_db(b"age")

        with self.env.begin() as txn:
            cursor = txn.cursor(self.people)
            self.next = _ID.unpack(cursor.key())[0] + 1 if cursor.last() else 1

    def insert(self, txn: lmdb.Transaction, values: dict[str, object]) -> None:
        id_ = self.next
        self.next += 1
        txn.put(_ID.pack(id_), encode(values), db=self.people)
        txn.put(str(values["email"]).encode(), _ID.pack(id_), db=self.email)
        txn.put(_AGE.pack(values["age"], id_), b"", db=self.age)

    def get(self, txn: lmdb.Transaction, id_: int) -> Person | None:
        data = txn.get(_ID.pack(id_), db=self.people)
        return None if data is None else decode(id_, data)

    def of_age(self, txn: lmdb.Transaction, age: int, limit: int, d: Digest) -> int:
        high = _AGE.pack(age + 1, 0)
        cursor = txn.cursor(self.age)
        given = 0

        if cursor.set_range(_AGE.pack(age, 0)):
            for key in cursor.iternext(keys=True, values=False):
                if given == limit or key >= high:
                    break
                d.person(self.get(txn, _AGE.unpack(key)[1]))
                given += 1

        return given


def run(directory: Path, rows: Rows) -> None:
    path = directory / "commits.lmdb"
    path.mkdir()
    store = Store(path, sync=True)

    def one(offset: int, r: int) -> None:
        with store.env.begin(write=True) as txn:
            store.insert(txn, person(offset + r))

    rows.each("insert-sync", 500, lambda r, d: one(0, r))
    store.env.close()
    store = Store(path, sync=False)
    rows.each("insert-deferred", 10_000, lambda r, d: one(1_000, r))
    store.env.sync(True)
    store.env.close()

    path = directory / "objects.lmdb"
    path.mkdir()
    store = Store(path, sync=True)

    def bulk(d: Digest) -> None:
        with store.env.begin(write=True) as txn:
            for n in range(OBJECTS):
                store.insert(txn, person(n))

    rows.all("insert-bulk", OBJECTS, bulk)

    ids = random_ids(OBJECTS)
    emails = [f"{n}@example.com".encode() for n in random_numbers(20_000)]

    with store.env.begin() as txn:

        def get_key(r: int, d: Digest) -> None:
            found = store.get(txn, ids[r])

            if found is not None:
                d.person(found)

        def get_email(r: int, d: Digest) -> None:
            key = txn.get(emails[r], db=store.email)

            if key is not None:
                d.person(store.get(txn, _ID.unpack(key)[0]))

        def age_between(r: int, d: Digest) -> None:
            low = r % 76
            left = 20

            for age in range(low + 4, low - 1, -1):
                left -= store.of_age(txn, age, left, d)

                if left == 0:
                    break

        def count(r: int, d: Digest) -> None:
            cursor = txn.cursor(store.age)
            n = 0

            if cursor.set_range(_AGE.pack(40, 0)):
                for _ in cursor.iternext(keys=True, values=False):
                    n += 1

            d.number(n)

        def city_scan(r: int, d: Digest) -> None:
            city = f"city {r % 100}".encode()

            for key, data in txn.cursor(store.people):
                if city_of(data) == city:
                    d.person(decode(_ID.unpack(key)[0], data))

        def top_score(r: int, d: Digest) -> None:
            top: list[tuple[float, int]] = []

            for key, data in txn.cursor(store.people):
                score = _HEAD.unpack_from(data)[1]
                id_ = _ID.unpack(key)[0]

                if len(top) == 10 and not (
                    score > top[9][0] or (score == top[9][0] and id_ < top[9][1])
                ):
                    continue

                at = next(
                    (
                        i
                        for i, (s, k) in enumerate(top)
                        if score > s or (score == s and id_ < k)
                    ),
                    len(top),
                )
                top.insert(at, (score, id_))
                del top[10:]

            for _, id_ in top:
                d.person(store.get(txn, id_))

        rows.each("get-key", OBJECTS, get_key)
        rows.each("get-email", 20_000, get_email)
        rows.each("age-equal", 200, lambda r, d: store.of_age(txn, r % 80, OBJECTS, d))
        rows.each("age-range", 5_000, age_between)
        rows.each("count", 200, count)
        rows.each("city-scan", 10, city_scan)
        rows.each("top-score", 10, top_score)

    def update(d: Digest) -> None:
        with store.env.begin(write=True) as txn:
            for r in range(10_000):
                id_ = ids[r]
                p = store.get(txn, id_)

                if p is not None:
                    txn.delete(_AGE.pack(p.age, id_), db=store.age)
                    p.age = (id_ + 1) % 80
                    txn.put(_ID.pack(id_), encode(p), db=store.people)
                    txn.put(_AGE.pack(p.age, id_), b"", db=store.age)

    def delete(d: Digest) -> None:
        with store.env.begin(write=True) as txn:
            for r in range(10_000):
                id_ = 1 + r * 7
                p = store.get(txn, id_)

                if p is not None:
                    txn.delete(_ID.pack(id_), db=store.people)
                    txn.delete(p.email.encode(), db=store.email)
                    txn.delete(_AGE.pack(p.age, id_), db=store.age)
                    d.number(1)

    rows.all("update", 10_000, update)
    rows.all("delete", 10_000, delete)

    left = Digest()

    with store.env.begin() as txn:
        for key, data in txn.cursor(store.people):
            left.person(decode(_ID.unpack(key)[0], data))

    rows.check("left", left)
    store.env.close()
