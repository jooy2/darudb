"""The notices of the third-party code in the native module of a wheel.

The module links the engine and every crate it and the binding depend on.
Their licences ask that their notices go with every copy, the binary
included. This collects them from `cargo metadata` and the licence files each
crate ships, and writes them into `python/darudb/THIRD_PARTY_NOTICES.txt`,
which maturin puts in the wheel beside the module, as `darudb/` holds it. The
file is git-ignored; the release workflow writes it before it builds a wheel.

    python tool/notices.py          write the file
    python tool/notices.py --print  print the notices instead

A crate is followed for every target a wheel is built for, so a crate linked
on one platform only is listed too, and one no target links is not.
Procedural macros run while the module compiles and are not in it, so neither
they nor what they depend on are listed. It is the same collection as
`packages/node/scripts/notices.mjs` makes for the Node.js addon.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

PACKAGE = Path(__file__).resolve().parent.parent
NOTICES = PACKAGE / "python" / "darudb" / "THIRD_PARTY_NOTICES.txt"

# The targets of the wheels the release workflow builds.
TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "i686-pc-windows-msvc",
]

# Folders of a crate where licence files belong to test data, not to it.
NOT_SOURCE = {"tests", "test", "benches", "examples", "target", ".git"}

# The names licence files go by.
LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|copyright|notice)", re.IGNORECASE)

# What a licence file names, when it names one of the licences below.
NAMED = {
    "MIT": re.compile("mit", re.IGNORECASE),
    "Apache-2.0": re.compile("apache", re.IGNORECASE),
    "Unicode-3.0": re.compile("unicode", re.IGNORECASE),
}


def linked_crates() -> list[dict[str, Any]]:
    """The crates the module links, with their metadata."""
    crates: dict[str, dict[str, Any]] = {}

    for target in TARGETS:
        metadata = json.loads(
            subprocess.run(
                [
                    "cargo",
                    "metadata",
                    "--format-version",
                    "1",
                    "--locked",
                    "--filter-platform",
                    target,
                ],
                cwd=PACKAGE,
                check=True,
                capture_output=True,
                text=True,
            ).stdout
        )
        packages = {found["id"]: found for found in metadata["packages"]}
        nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
        root = next(found for found in metadata["packages"] if found["name"] == "darudb-python")
        stack = [root["id"]]
        seen: set[str] = set()

        while stack:
            identifier = stack.pop()
            found = packages[identifier]

            if identifier in seen or any(
                "proc-macro" in built["kind"] for built in found["targets"]
            ):
                continue

            seen.add(identifier)

            # The workspace's own packages are under this package's licence.
            if found["source"] is not None:
                crates[identifier] = found

            for dep in nodes[identifier]["deps"]:
                if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                    stack.append(dep["pkg"])

    return sorted(crates.values(), key=lambda crate: (crate["name"], crate["version"]))


def licence_files(directory: Path, depth: int = 0) -> list[Path]:
    """Every licence file in ``directory``, searched three folders deep."""
    files: list[Path] = []

    for entry in sorted(directory.iterdir()):
        if entry.is_dir():
            if depth < 3 and entry.name not in NOT_SOURCE:
                files.extend(licence_files(entry, depth + 1))
        elif LICENCE_FILE.match(entry.name):
            files.append(entry)

    return files


def licences_used(expression: str) -> list[str]:
    """The licences a crate is used under: of each alternative its expression
    offers, MIT where it is one, and the first otherwise."""
    terms: list[str] = []
    depth = 0
    start = 0

    for at, character in enumerate(expression):
        depth += 1 if character == "(" else -1 if character == ")" else 0

        if depth == 0 and expression.startswith(" AND ", at):
            terms.append(expression[start:at])
            start = at + len(" AND ")

    terms.append(expression[start:])
    used = []

    for term in terms:
        choices = [choice.strip() for choice in re.sub(r"[()]", "", term).split(" OR ")]
        used.append("MIT" if "MIT" in choices else choices[0])

    return used


def files_used(files: list[Path], used: list[str]) -> list[Path]:
    """The licence files that the licences a crate is used under need."""
    notice = re.compile(r"^(copyright|notice)", re.IGNORECASE)
    general = [
        file
        for file in files
        if not notice.match(file.name)
        and not any(pattern.search(file.name) for pattern in NAMED.values())
    ]
    chosen = {file for file in files if notice.match(file.name)}

    for licence in used:
        pattern = NAMED.get(licence)
        named = [file for file in files if pattern is not None and pattern.search(file.name)]
        chosen.update(named or general)

    return sorted(chosen)


def notices() -> str:
    """The notices of every crate the module links."""
    sections = []

    for crate in linked_crates():
        directory = Path(crate["manifest_path"]).parent
        used = licences_used(crate.get("license") or "")
        texts = [
            (file.relative_to(directory).as_posix(), file.read_text(encoding="utf-8"))
            for file in files_used(licence_files(directory), used)
        ]

        if not texts:
            raise SystemExit(f"{crate['name']} {crate['version']} ships no licence file")

        lines = [
            "=" * 80,
            f"{crate['name']} {crate['version']}",
            f"Licence used: {' AND '.join(used)}, of {crate['license']}",
        ]

        if crate.get("repository"):
            lines.append(f"Source: {crate['repository']}")

        for name, text in texts:
            lines.extend(["", f"--- {name} ---", "", text.replace("\r\n", "\n").rstrip()])

        sections.append("\n".join(lines))

    return "\n".join(
        [
            "Third-party notices",
            "",
            "The native module of DaruDB in this package is compiled from DaruDB's own",
            "source, under the licence in the package's LICENSE, and from the Rust crates",
            "below, which are linked into it. Each is listed with the licence it is used",
            "under and the licence files it ships.",
            "",
            *sections,
            "",
        ]
    )


def main() -> None:
    text = notices()

    if "--print" in sys.argv:
        sys.stdout.write(text)
    else:
        NOTICES.write_text(text, encoding="utf-8")
        print(f"Notices written into {NOTICES.relative_to(PACKAGE)}.")


if __name__ == "__main__":
    main()
