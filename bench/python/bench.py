"""The Python side of the benchmark of bench/README.md: DaruDB and the
embedded stores Python programs use most, each running the same workloads.

  python bench.py --stores                   the stores, with their versions, as JSON
  python bench.py --child STORE --dir DIR    one pass of one store, one line of JSON per row

bench/run.mjs runs the passes, each in a process of its own on new files, and
puts the runs together."""

from __future__ import annotations

import json
import sqlite3
import sys
from importlib.metadata import version
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Rows


def main(args: list[str]) -> None:
    if "--stores" in args:
        print(
            json.dumps(
                [
                    {"id": "daru", "version": version("darudb")},
                    {
                        "id": "sqlite",
                        "version": f"{sqlite3.sqlite_version} (sqlite3 module)",
                    },
                    {"id": "lmdb", "version": f"py-lmdb {version('lmdb')}"},
                ]
            )
        )
        return

    store = args[args.index("--child") + 1]
    directory = Path(args[args.index("--dir") + 1])
    directory.mkdir(parents=True, exist_ok=True)
    rows = Rows()

    if store == "daru":
        import daru_run

        daru_run.run(directory, rows)
    elif store == "sqlite":
        import sqlite_run

        sqlite_run.run(directory, rows)
    elif store == "lmdb":
        import lmdb_run

        lmdb_run.run(directory, rows)
    else:
        raise SystemExit(f"no store is named {store}")

    rows.finish()


if __name__ == "__main__":
    main(sys.argv[1:])
