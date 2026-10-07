"""Threads and processes on one file."""

from __future__ import annotations

import multiprocessing
import os
import sys
import threading
import time
from pathlib import Path

import pytest

from darudb import DaruError
from models import User, open_db


def test_threads_read_while_another_thread_holds_the_write(path: Path) -> None:
    with open_db(path) as db:
        with db.write() as txn:
            txn.collection(User).insert(User(name="Alice"))

        holding = threading.Event()
        release = threading.Event()

        def writer() -> None:
            with db.write() as txn:
                txn.collection(User).insert(User(name="Bob"))
                holding.set()
                release.wait(10)

        thread = threading.Thread(target=writer)
        thread.start()
        holding.wait(10)

        # The writer holds the GIL-free engine call open; reads go on meanwhile.
        counts = []

        def reader() -> None:
            with db.read() as txn:
                counts.append(txn.collection(User).count())

        readers = [threading.Thread(target=reader) for _ in range(8)]

        for thread_ in readers:
            thread_.start()

        for thread_ in readers:
            thread_.join(10)

        release.set()
        thread.join(10)

        assert counts == [1] * 8

        with db.read() as txn:
            assert txn.collection(User).count() == 2


def test_a_writer_waits_for_another_thread_and_then_goes_on(path: Path) -> None:
    with open_db(path) as db:
        started = threading.Event()
        order: list[str] = []

        def first() -> None:
            with db.write() as txn:
                started.set()
                time.sleep(0.2)
                txn.collection(User).insert(User(name="first"))
                order.append("first")

        thread = threading.Thread(target=first)
        thread.start()
        started.wait(10)

        with db.write() as txn:
            txn.collection(User).insert(User(name="second"))
            order.append("second")

        thread.join(10)

        assert order == ["first", "second"]


def test_many_threads_write_and_every_commit_lands(path: Path) -> None:
    with open_db(path) as db:

        def work(n: int) -> None:
            for m in range(20):
                with db.write(durability="deferred") as txn:
                    txn.collection(User).insert(User(name=f"{n}-{m}"))

        threads = [threading.Thread(target=work, args=(n,)) for n in range(8)]

        for thread in threads:
            thread.start()

        for thread in threads:
            thread.join(60)

        with db.read() as txn:
            assert txn.collection(User).count() == 160


def _worker(path: str, n: int) -> None:
    sys.path.insert(0, str(Path(__file__).parent))

    from models import User as WorkerUser
    from models import open_db as worker_open

    with worker_open(path, busy_timeout=30.0) as db:
        for m in range(25):
            with db.write() as txn:
                txn.collection(WorkerUser).insert(WorkerUser(name=f"{n}-{m}"))


def test_processes_write_one_file(path: Path) -> None:
    open_db(path).close()
    context = multiprocessing.get_context("spawn")
    workers = [context.Process(target=_worker, args=(str(path), n)) for n in range(4)]

    for worker in workers:
        worker.start()

    for worker in workers:
        worker.join(120)
        assert worker.exitcode == 0

    with open_db(path) as db:
        assert db.check().ok

        with db.read() as txn:
            assert txn.collection(User).count() == 100


@pytest.mark.skipif(not hasattr(os, "fork"), reason="the system has no fork")
# The engine's threads, and the asynchronous API's pool from the tests before,
# may still run, which Python warns about on Linux.
@pytest.mark.filterwarnings("ignore:.*multi-threaded.*fork:DeprecationWarning")
def test_a_forked_child_opens_the_file_again(path: Path) -> None:
    db = open_db(path)

    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))

    read, write = os.pipe()
    child = os.fork()

    if child == 0:
        status = 1

        try:
            os.close(read)

            # The parent's handle holds none of the file's locks here.
            try:
                with db.read():
                    pass
            except DaruError as error:
                closed = error.code == "CLOSED"
            else:
                closed = False

            with open_db(path) as own, own.write() as txn:
                txn.collection(User).insert(User(name="Bob"))

            os.write(write, b"ok" if closed else b"no")
            status = 0
        finally:
            os._exit(status)

    os.close(write)
    answer = os.read(read, 2)
    os.close(read)
    _, status = os.waitpid(child, 0)

    assert answer == b"ok"
    assert os.waitstatus_to_exitcode(status) == 0

    with db.read() as txn:
        assert txn.collection(User).count() == 2

    db.close()
