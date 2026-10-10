"""What every store runs: the objects, the order random reads go in, timing,
and the digest a row's results are checked with. bench/README.md describes
the workloads, and the other languages' harnesses draw the same objects in
the same order."""

from __future__ import annotations

import json
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass

OBJECTS = 100_000

_MASK = (1 << 64) - 1


@dataclass(slots=True)
class Person:
    """An object as the key-value stores and SQL give it back, made by hand."""

    id: int
    name: str
    email: str
    age: int
    city: str
    score: float


def person(n: int) -> dict[str, object]:
    """The ``n``th object, counting from 0. It gets the key ``n + 1``."""
    return {
        "name": f"person {n}",
        "email": f"{n}@example.com",
        "age": n * 7919 % 80,
        "city": f"city {n % 100}",
        "score": (n * 0.618) % 1,
    }


def _scatter(round_: int) -> int:
    x = (round_ * 0x9E3779B97F4A7C15) & _MASK
    return ((x << 17) | (x >> 47)) & _MASK


def random_ids(count: int) -> list[int]:
    """The keys the random reads ask for, drawn before the timing starts."""
    return [1 + _scatter(r) % OBJECTS for r in range(count)]


def random_numbers(count: int) -> list[int]:
    """The numbers of the objects the random email lookups ask for."""
    return [_scatter(r) % OBJECTS for r in range(count)]


class Digest:
    """How many results a row saw, and a 32-bit hash of their keys and ages."""

    __slots__ = ("count", "hash")

    def __init__(self) -> None:
        self.count = 0
        self.hash = 0

    def add(self, id_: int, age: int) -> None:
        self.count += 1
        self.hash = (
            (self.hash ^ ((id_ * 131 + age) & 0xFFFFFFFF)) * 0x01000193
        ) & 0xFFFFFFFF

    def person(self, p: object) -> None:
        self.add(p.id, p.age)  # type: ignore[attr-defined]

    def number(self, n: int) -> None:
        self.add(n, 0)


class Rows:
    def __init__(self) -> None:
        self.rows: list[dict[str, object]] = []

    def each(self, row: str, count: int, step: Callable[[int, Digest], None]) -> None:
        """Runs ``step`` ``count`` times and records the time each took on average."""
        digest = Digest()
        started = time.perf_counter_ns()

        for round_ in range(count):
            step(round_, digest)

        self._push(row, (time.perf_counter_ns() - started) / count, digest)

    def all(self, row: str, count: int, work: Callable[[Digest], None]) -> None:
        """Runs ``work`` once, which does ``count`` operations and commits them."""
        digest = Digest()
        started = time.perf_counter_ns()

        work(digest)
        self._push(row, (time.perf_counter_ns() - started) / count, digest)

    def check(self, row: str, digest: Digest) -> None:
        """A row that is checked and not timed."""
        self._push(row, 0, digest)

    def _push(self, row: str, ns: float, digest: Digest) -> None:
        self.rows.append(
            {"row": row, "ns": ns, "count": digest.count, "hash": digest.hash}
        )

    def finish(self) -> None:
        """Writes the rows for the parent, one line of JSON each."""
        sys.stdout.write("".join(json.dumps(row) + "\n" for row in self.rows))
