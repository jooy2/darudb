"""The integrity check, backup, compaction and salvage."""

from __future__ import annotations

from pathlib import Path

import pytest

import darudb
from darudb import DaruError, PasswordHashing
from models import User, open_db

CHEAP = PasswordHashing(memory_kib=64, iterations=1, parallelism=1)


def fill(db: darudb.Database, count: int) -> None:
    with db.write() as txn:
        txn.collection(User).insert_many(User(name=f"user {n}", age=n) for n in range(count))


def test_check_reports_a_sound_file(path: Path) -> None:
    with open_db(path) as db:
        fill(db, 100)
        report = db.check()

    assert report.ok
    assert report.problems == ()
    assert report.objects_checked == 100
    assert report.pages_checked > 0
    assert report.page_count >= report.pages_checked
    assert report.commit_id > 0


def test_check_reports_the_damage_it_finds(path: Path) -> None:
    with open_db(path) as db:
        fill(db, 2000)

    data = bytearray(path.read_bytes())
    page = 4096 * 5
    data[page + 100 : page + 140] = bytes(40)
    path.write_bytes(bytes(data))

    # Page 5 is a leaf of an index, which opening the file never reads.
    with open_db(path) as db:
        report = db.check()

    assert not report.ok
    assert report.problems[0].page == 5
    assert "fails its check" in report.problems[0].message


def test_backup_copies_the_published_commit(path: Path, tmp_path: Path) -> None:
    copy = tmp_path / "copy.darudb"

    with open_db(path) as db:
        fill(db, 50)
        report = db.backup(copy)

        with pytest.raises(DaruError) as error:
            db.backup(copy)

        assert error.value.code == "INVALID_ARGUMENT"

    assert report.bytes == copy.stat().st_size
    assert report.entries > 50

    with open_db(copy) as db, db.read() as txn:
        assert txn.collection(User).count() == 50


def test_backup_with_a_password_encrypts_the_copy(path: Path, tmp_path: Path) -> None:
    copy = tmp_path / "copy.darudb"

    with open_db(path) as db:
        fill(db, 5)
        db.backup(copy, password="for the copy", password_hashing=CHEAP)

    with pytest.raises(DaruError) as error:
        open_db(copy)

    assert error.value.code == "KEY_REQUIRED"

    with open_db(copy, password="for the copy") as db:
        assert db.is_encrypted


def test_compact_gives_space_back(path: Path) -> None:
    with open_db(path) as db:
        fill(db, 3000)

        with db.write() as txn:
            users = txn.collection(User)

            for key in range(1, 2900):
                users.delete(key)

        report = db.compact()

        assert report.bytes_after <= report.bytes_before
        assert report.bytes_after == path.stat().st_size
        assert db.check().ok


def test_salvage_rescues_a_closed_file_and_needs_it_alone(path: Path, tmp_path: Path) -> None:
    target = tmp_path / "saved.darudb"

    with open_db(path) as db:
        fill(db, 20)

        with pytest.raises(DaruError) as error:
            darudb.Database.salvage(path, target)

        assert error.value.code == "BUSY"

    report = darudb.Database.salvage(path, target)

    assert report.whole
    assert report.objects_dropped == 0
    assert report.bytes == target.stat().st_size

    with open_db(target) as db:
        assert db.check().ok

        with db.read() as txn:
            assert txn.collection(User).count() == 20


def test_salvage_of_an_encrypted_file_takes_its_key(path: Path, tmp_path: Path) -> None:
    key = bytes(range(32))

    with open_db(path, key=key) as db:
        fill(db, 3)

    report = darudb.Database.salvage(path, tmp_path / "saved.darudb", key=key)

    assert report.whole
