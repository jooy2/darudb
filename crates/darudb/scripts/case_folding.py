"""Writes `src/query/fold_table.rs`, the simple case folding of Unicode, from
the Unicode Character Database's `CaseFolding.txt`.

A comparison that ignores case folds each character of both strings and
compares the folded strings. Simple case folding maps one character to one
character: the mappings of status C, common to every folding, and S, the
simple ones. The full mappings of status F, which make one character several
(`ß` to `ss`), and the Turkic ones of status T are left out, so that a string
folds to a string of the same number of characters, which the engine folds as
it compares, without allocating.

Run it from `crates/darudb` with the file of the Unicode version to use:

    curl -sSfLO https://www.unicode.org/Public/18.0.0/ucd/CaseFolding.txt
    python3 scripts/case_folding.py CaseFolding.txt

then `cargo fmt`. A new version only adds mappings for characters it adds:
Unicode keeps the folding of characters already assigned stable.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src" / "query" / "fold_table.rs"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("usage: case_folding.py CaseFolding.txt")

    text = Path(sys.argv[1]).read_text(encoding="utf-8")
    version = re.match(r"# CaseFolding-(\d+\.\d+\.\d+)\.txt", text)

    if version is None:
        sys.exit("the file does not say which version of CaseFolding.txt it is")

    folds: list[tuple[int, int]] = []

    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()

        if not line:
            continue

        code, status, mapping, _ = (part.strip() for part in line.split(";"))

        if status in ("C", "S"):
            targets = mapping.split()

            if len(targets) != 1:
                sys.exit(f"{code} has a simple folding to several characters")

            folds.append((int(code, 16), int(targets[0], 16)))

    folds.sort()

    for (a, _), (b, _) in zip(folds, folds[1:]):
        if a == b:
            sys.exit(f"{a:04X} folds two ways")

    lines = [
        "//! Unicode's simple case folding: each character that folds to another,",
        "//! and the one it folds to, by the first. Written by",
        f"//! `scripts/case_folding.py` from `CaseFolding.txt` of Unicode {version[1]}",
        "//! (statuses C and S); do not edit it by hand.",
        "//!",
        "//! The data is Unicode's, © Unicode, Inc., used under the Unicode License",
        "//! v3, whose notice is in `LICENSE-UNICODE` beside this crate's licence.",
        "",
        "/// Each character that folds to another, and the one it folds to, sorted",
        "/// by the first.",
        "pub(crate) const FOLDS: &[(char, char)] = &[",
    ]
    lines += [f"    ('\\u{{{a:04X}}}', '\\u{{{b:04X}}}')," for a, b in folds]
    lines += ["];", ""]
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"{len(folds)} foldings of Unicode {version[1]} written to {OUT}")


if __name__ == "__main__":
    main()
