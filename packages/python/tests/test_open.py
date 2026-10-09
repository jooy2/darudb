"""Opening a file: options, properties, closing, and the schema check."""

from __future__ import annotations

import re
from pathlib import Path

import pytest

import darudb
from darudb import DaruError, field
from models import SCHEMA, User, open_db


def test_open_creates_a_database_and_describes_it(path: Path) -> None:
    db = open_db(path)

    assert path.exists()
    assert db.path == str(path)
    assert db.is_open
    assert db.page_size == 4096
    assert db.format_version == darudb.FORMAT_VERSION
    assert not db.is_encrypted
    assert db.schema is SCHEMA
    assert db.schema_version == 1
    assert repr(db) == f"<Database {str(path)!r} open>"

    db.close()

    assert not db.is_open
    assert db.path == str(path)


def test_a_database_without_a_schema_has_no_collections(path: Path) -> None:
    with darudb.Database.open(path) as db:
        assert db.schema_version is None

        with db.read() as txn, pytest.raises(DaruError) as error:
            txn.collection(User)

        assert error.value.code == "INVALID_ARGUMENT"


def test_create_false_refuses_a_missing_file(path: Path) -> None:
    with pytest.raises(DaruError) as error:
        open_db(path, create=False)

    assert error.value.code == "NOT_FOUND"
    assert not path.exists()


def test_options_out_of_range_are_refused(path: Path) -> None:
    for options in ({"page_size": 1000}, {"busy_timeout": -1.0}, {"busy_timeout": float("nan")}):
        with pytest.raises(DaruError) as error:
            open_db(path, **options)

        assert error.value.code == "INVALID_ARGUMENT", options


def test_a_page_size_is_kept_by_the_file(path: Path) -> None:
    open_db(path, page_size=16384).close()

    with open_db(path) as db:
        assert db.page_size == 16384


def test_a_closed_database_refuses_work_and_closes_again_quietly(path: Path) -> None:
    db = open_db(path)
    db.close()
    db.close()

    with pytest.raises(DaruError) as error:
        db.read().__enter__()

    assert error.value.code == "CLOSED"

    for use in (lambda: db.page_size, db.check, db.sync, db.compact):
        with pytest.raises(DaruError) as error:
            use()

        assert error.value.code == "CLOSED"


def test_a_database_is_a_context_manager_that_closes_it(path: Path) -> None:
    with open_db(path) as db:
        assert db.is_open

    assert not db.is_open


def test_database_has_no_constructor() -> None:
    with pytest.raises(TypeError):
        darudb.Database()


def test_the_same_schema_opens_again_and_a_changed_one_needs_a_version(path: Path) -> None:
    open_db(path).close()
    open_db(path).close()

    @darudb.collection("users")
    class Changed:
        id: int | None = None
        name: str
        nickname: str | None = None

    with pytest.raises(DaruError) as error:
        darudb.Database.open(path, schema=darudb.Schema(1, [Changed]))

    assert error.value.code == "SCHEMA_MISMATCH"


def test_a_file_with_a_newer_schema_is_refused(path: Path) -> None:
    @darudb.collection("users")
    class Later:
        id: int | None = None
        name: str
        nickname: str | None = None

    darudb.Database.open(path, schema=darudb.Schema(2, [Later])).close()

    with pytest.raises(DaruError) as error:
        open_db(path)

    assert error.value.code == "SCHEMA_TOO_NEW"


def test_two_handles_to_one_file_share_its_commits(path: Path) -> None:
    with open_db(path) as first, open_db(path) as second:
        with first.write() as txn:
            txn.collection(User).insert(User(name="Alice"))

        with second.read() as txn:
            assert txn.collection(User).count() == 1


def test_a_non_database_file_is_refused(path: Path) -> None:
    path.write_bytes(b"not a database" * 1000)

    with pytest.raises(DaruError) as error:
        open_db(path)

    assert error.value.code == "NOT_A_DATABASE"


def test_errors_carry_the_code_beside_the_message(path: Path) -> None:
    with pytest.raises(DaruError) as error:
        open_db(path, create=False)

    assert str(error.value) == f"NOT_FOUND: {error.value.message}"
    assert repr(error.value).startswith("DaruError('NOT_FOUND', ")
    assert error.value.args == ("NOT_FOUND", error.value.message)


def test_the_versions_match_the_package() -> None:
    pyproject = (Path(__file__).parent.parent / "pyproject.toml").read_text()
    version = re.search(r'^version = "([^"]+)"', pyproject, re.MULTILINE)

    assert version is not None
    assert darudb.__version__ == version.group(1)
    assert re.fullmatch(r"\d+\.\d+\.\d+", darudb.ENGINE_VERSION)
    assert darudb.FORMAT_VERSION == 6


def test_a_schema_refuses_what_is_not_a_collection() -> None:
    @darudb.embedded
    class Part:
        name: str

    with pytest.raises(DaruError) as error:
        darudb.Schema(1, [Part])

    assert error.value.code == "INVALID_ARGUMENT"

    @darudb.collection("users")
    class Twin:
        id: int | None = None
        name: str = field(default="")

    with pytest.raises(DaruError):
        darudb.Schema(1, [User, Twin])


def test_options_of_the_wrong_kind_are_invalid(path: Path) -> None:
    for options in (
        {"page_size": "4096"},
        {"page_size": -1},
        {"cache_size": -1},
        {"cache_size": 1.5},
        {"busy_timeout": "5"},
        {"busy_timeout": float("inf")},
        {"create": 1},
        {"schema": "users"},
        {"password_hashing": (1, 1, 1)},
    ):
        with pytest.raises(DaruError) as error:
            open_db(path, **options)

        assert error.value.code == "INVALID_ARGUMENT", options

    assert not path.exists()

    for make in (
        lambda: darudb.Migration(-1),
        lambda: darudb.Migration(2.5),  # type: ignore[arg-type]
        lambda: darudb.Migration(2, rename_fields=[("users", "a")]),  # type: ignore[list-item]
        lambda: darudb.Migration(2, delete_collections="users"),
        lambda: darudb.Migration(2, run="later"),  # type: ignore[arg-type]
    ):
        with pytest.raises(DaruError) as error:
            make()

        assert error.value.code == "INVALID_ARGUMENT"
