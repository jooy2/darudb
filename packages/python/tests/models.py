"""The classes the tests store, shared by every test module."""

from __future__ import annotations

import darudb
from darudb import field


@darudb.embedded
class Address:
    city: str
    zip: str | None = field(default=None, name="postcode")


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    rating: float = 0.0
    active: bool = True
    tags: list[str] = field(default_factory=list, index=True)
    avatar: bytes | None = None
    address: Address | None = None


@darudb.collection("posts")
class Post:
    slug: str = field(primary_key=True)
    author: int = field(link=User, index=True)
    readers: list[int] = field(link=User, default_factory=list)
    title: str = ""


@darudb.collection("blobs")
class Blob:
    digest: bytes = field(primary_key=True)
    size: int


SCHEMA = darudb.Schema(1, [User, Post, Blob])


def open_db(path: object, **options: object) -> darudb.Database:
    """The tests' database at ``path``, with their schema unless told otherwise."""
    options.setdefault("schema", SCHEMA)

    return darudb.Database.open(path, **options)  # type: ignore[arg-type]
