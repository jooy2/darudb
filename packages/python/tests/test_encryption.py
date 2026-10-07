"""Encrypted files: keys, passwords, and changing them."""

from __future__ import annotations

from pathlib import Path

import pytest

import darudb
from darudb import DaruError, PasswordHashing
from models import User, open_db

KEY = bytes(range(32))
OTHER = bytes(32)
# Cheap enough to keep the tests fast; a file records its own cost.
CHEAP = PasswordHashing(memory_kib=64, iterations=1, parallelism=1)


def fill(path: Path, **options: object) -> None:
    with open_db(path, **options) as db, db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))


def count(path: Path, **options: object) -> int:
    with open_db(path, **options) as db, db.read() as txn:
        return txn.collection(User).count()


def test_a_key_encrypts_a_new_file_and_opens_it(path: Path) -> None:
    fill(path, key=KEY)

    assert b"Alice" not in path.read_bytes()
    assert count(path, key=bytearray(KEY)) == 1

    with open_db(path, key=memoryview(KEY)) as db:
        assert db.is_encrypted


def test_a_password_encrypts_a_new_file_and_opens_it(path: Path) -> None:
    fill(path, password="correct horse", password_hashing=CHEAP)

    assert count(path, password="correct horse") == 1
    assert count(path, password=b"correct horse") == 1


def test_the_wrong_secret_and_no_secret_are_refused(path: Path) -> None:
    fill(path, key=KEY)

    for options, code in (
        ({}, "KEY_REQUIRED"),
        ({"key": OTHER}, "WRONG_KEY"),
        ({"password": "guess", "password_hashing": CHEAP}, "WRONG_KEY"),
    ):
        with pytest.raises(DaruError) as error:
            open_db(path, **options)

        assert error.value.code == code, options


def test_a_key_is_32_bytes_and_a_plain_file_takes_none(path: Path) -> None:
    for key in (b"short", "a string of thirty-two characters", 42):
        with pytest.raises(DaruError) as error:
            open_db(path, key=key)

        assert error.value.code == "INVALID_ARGUMENT", key

    fill(path)

    with pytest.raises(DaruError) as error:
        open_db(path, key=KEY)

    assert error.value.code == "INVALID_ARGUMENT"

    with pytest.raises(DaruError) as error:
        open_db(path, password="")

    assert error.value.code == "INVALID_ARGUMENT"


def test_set_key_and_set_password_change_what_opens_the_file(path: Path) -> None:
    fill(path, key=KEY, password_hashing=CHEAP)

    with open_db(path, key=KEY) as db:
        db.set_key(OTHER)

    with pytest.raises(DaruError):
        open_db(path, key=KEY)

    with open_db(path, key=OTHER) as db:
        db.set_password("new password")

    assert count(path, password="new password") == 1


def test_a_plain_file_has_no_key_to_change(path: Path) -> None:
    with open_db(path) as db:
        assert not db.is_encrypted

        with pytest.raises(DaruError) as error:
            db.set_key(KEY)

        assert error.value.code == "INVALID_ARGUMENT"


def test_a_key_and_a_password_together_are_refused(path: Path, tmp_path: Path) -> None:
    with pytest.raises(DaruError) as error:
        open_db(path, key=KEY, password="both", password_hashing=CHEAP)

    assert error.value.code == "INVALID_ARGUMENT"
    assert not path.exists()

    fill(path, key=KEY)

    with open_db(path, key=KEY) as db, pytest.raises(DaruError) as error:
        db.backup(tmp_path / "copy.darudb", key=OTHER, password="both")

    assert error.value.code == "INVALID_ARGUMENT"

    with pytest.raises(DaruError) as error:
        darudb.Database.salvage(path, tmp_path / "saved.darudb", key=KEY, password="both")

    assert error.value.code == "INVALID_ARGUMENT"
