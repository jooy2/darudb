"""Writes `src/format/object/word_table.rs`, the characters a text index
makes words of, from the Unicode Character Database.

A text index splits a string into words. A character belongs to a word when
it is a letter, a digit or a mark: `Alphabetic` in
`DerivedCoreProperties.txt`, or of the general category `Nd`, `Mn`, `Mc` or
`Me` in `DerivedGeneralCategory.txt`. Everything else separates words. The
characters of the scripts written without spaces between words, `Han`,
`Hiragana` and `Katakana` in `Scripts.txt`, with the two prolonged sound
marks those scripts share, are listed apart: the index takes them two at a
time rather than as words.

The standard library has such tables too, but of the Unicode version of the
compiler that builds the engine, and an index written by one build has to
hold the words another build looks for.

Run it from `crates/darudb` with the files of the Unicode version to use,
the one `fold_table.rs` was made from:

    curl -sSfLO https://www.unicode.org/Public/18.0.0/ucd/DerivedCoreProperties.txt
    curl -sSfLO https://www.unicode.org/Public/18.0.0/ucd/extracted/DerivedGeneralCategory.txt
    curl -sSfLO https://www.unicode.org/Public/18.0.0/ucd/Scripts.txt
    python3 scripts/word_chars.py DerivedCoreProperties.txt DerivedGeneralCategory.txt Scripts.txt

then `cargo fmt`. A later version of Unicode adds characters, and a
character that was a separator in a file's index is a letter to the new
build, so a table of another version needs the text indexes built again.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src" / "format" / "object" / "word_table.rs"

CATEGORIES = {"Nd", "Mn", "Mc", "Me"}
SCRIPTS = {"Han", "Hiragana", "Katakana"}

# The prolonged sound marks, whose script is Common since hiragana and
# katakana share them: a word such as U+30B3 U+30FC U+30D2 U+30FC would
# otherwise end at each.
SHARED = {0x30FC, 0xFF70}


def version_of(text: str, name: str) -> str:
    version = re.match(rf"# {name}-(\d+\.\d+\.\d+)\.txt", text)

    if version is None:
        sys.exit(f"the file does not say which version of {name}.txt it is")

    return version[1]


def having(text: str, wanted: set[str]) -> set[int]:
    """The code points a file of `range ; value` lines gives a wanted value."""
    found: set[int] = set()

    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()

        if not line:
            continue

        span, value = (part.strip() for part in line.split(";")[:2])

        if value in wanted:
            first, _, last = span.partition("..")
            found.update(range(int(first, 16), int(last or first, 16) + 1))

    return found


def ranges(points: set[int]) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []

    for point in sorted(points):
        if out and out[-1][1] + 1 == point:
            out[-1] = (out[-1][0], point)
        else:
            out.append((point, point))

    return out


def table(name: str, doc: list[str], spans: list[tuple[int, int]]) -> list[str]:
    lines = [f"/// {line}" for line in doc]
    lines.append(f"pub(super) const {name}: &[(u32, u32)] = &[")
    lines += [f"    (0x{first:04X}, 0x{last:04X})," for first, last in spans]
    lines += ["];", ""]

    return lines


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit(
            "usage: word_chars.py DerivedCoreProperties.txt DerivedGeneralCategory.txt Scripts.txt"
        )

    core, category, scripts = (Path(path).read_text(encoding="utf-8") for path in sys.argv[1:])
    versions = {
        version_of(core, "DerivedCoreProperties"),
        version_of(category, "DerivedGeneralCategory"),
        version_of(scripts, "Scripts"),
    }

    if len(versions) != 1:
        sys.exit("the files are of different versions of Unicode")

    version = versions.pop()
    words = having(core, {"Alphabetic"}) | having(category, CATEGORIES)
    pairs = (having(scripts, SCRIPTS) | SHARED) & words
    # ASCII is told apart without the table.
    words = {point for point in words - pairs if point > 0x7F}

    lines = [
        "//! The characters a text index makes words of, as ranges of code points.",
        "//! Written by `scripts/word_chars.py` from `DerivedCoreProperties.txt`,",
        f"//! `DerivedGeneralCategory.txt` and `Scripts.txt` of Unicode {version}; do not",
        "//! edit it by hand.",
        "//!",
        "//! The data is Unicode's, © Unicode, Inc., used under the Unicode License",
        "//! v3, whose notice is in `LICENSE-UNICODE` beside this crate's licence.",
        "",
    ]
    lines += table(
        "WORDS",
        [
            "The letters, digits and marks past ASCII that are not in [`PAIRS`]:",
            "each range's first and last code point, sorted.",
        ],
        ranges(words),
    )
    lines += table(
        "PAIRS",
        [
            "The letters of the scripts written without spaces between words, Han,",
            "hiragana and katakana: each range's first and last code point, sorted.",
        ],
        ranges(pairs),
    )
    OUT.write_text("\n".join(lines).rstrip("\n") + "\n", encoding="utf-8")
    print(f"{len(ranges(words))} and {len(ranges(pairs))} ranges of Unicode {version} written to {OUT}")


if __name__ == "__main__":
    main()
