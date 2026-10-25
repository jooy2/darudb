"""What every store runs in the complex suite: three collections that link to
each other, the objects in them, and the parameters of each row's runs.
bench/README.md describes the suite, and the other languages' harnesses draw
the same objects and parameters from the same hash."""

from __future__ import annotations

ORGANIZATIONS = 1_000
PEOPLE = 100_000
POSTS = 300_000

# Objects a write transaction of the load inserts.
BATCH = 10_000

KINDS = ["company", "school", "hospital", "agency", "club"]

COUNTRIES = [
    "kr", "us", "jp", "de", "fr", "gb", "cn", "in", "br", "ca",
    "au", "es", "it", "nl", "se", "mx", "id", "tr", "pl", "vn",
]  # fmt: skip

LANGUAGES = ["en", "ko", "ja", "de", "fr", "es"]

TAGS = [
    "travel", "food", "music", "sport", "news", "tech", "art", "books", "games", "film",
    "health", "science", "nature", "design", "fashion", "history", "photo", "pets", "cars", "money",
]  # fmt: skip

WORDS = [
    "able", "about", "above", "across", "after", "again", "air", "almost", "along", "always",
    "animal", "answer", "around", "away", "back", "before", "began", "below", "between", "black",
    "body", "book", "both", "bring", "build", "call", "came", "carry", "change", "child", "city",
    "close", "color", "come", "country", "cover", "cross", "dark", "day", "deep", "door", "draw",
    "early", "earth", "east", "end", "even", "every", "face", "fall", "family", "far", "farm",
    "fast", "field", "fire", "first", "food", "foot", "form", "found", "free", "friend", "full",
]  # fmt: skip

_MASK = 0xFFFFFFFF


def _hash(x: int) -> int:
    """A 32-bit integer hash, the same in every language's harness."""
    x = (((x >> 16) ^ x) * 0x045D9F3B) & _MASK
    x = (((x >> 16) ^ x) * 0x045D9F3B) & _MASK
    return (x >> 16) ^ x


def mix(n: int, salt: int) -> int:
    """The value ``salt`` draws for the ``n``th object or run."""
    return _hash((_hash(n & _MASK) + salt) & _MASK)


def _pick(values: list[str], n: int, salt: int) -> str:
    return values[mix(n, salt) % len(values)]


def _tags(count: int, first: int) -> list[str]:
    return [TAGS[(first % 20 + at * 7) % 20] for at in range(count)]


def code(n: int) -> str:
    """The code of the ``n``th organization."""
    return f"org-{n:04}"


def organization(n: int) -> dict[str, object]:
    return {
        "code": code(n),
        "name": f"organization {n}",
        "kind": _pick(KINDS, n, 1),
        "country": _pick(COUNTRIES, n, 2),
        "founded": 1900 + mix(n, 3) % 120,
    }


def person(n: int) -> dict[str, object]:
    """The ``n``th person, counting from 0. They get the key ``n + 1``."""
    return {
        "name": f"person {n}",
        "email": f"{n}@example.com",
        "age": mix(n, 1) % 80,
        "country": _pick(COUNTRIES, n, 2),
        "city": f"city {mix(n, 3) % 500}",
        "zip": f"{mix(n, 4) % 100_000:05}",
        "score": (mix(n, 5) % 1_000_000) / 1_000_000,
        "active": mix(n, 6) % 7 != 0,
        # A permutation of the people, so that no two join at once.
        "joined_at": 1_600_000_000 + (n * 7_907 % PEOPLE) * 37,
        "organization": code(mix(n, 7) % ORGANIZATIONS),
        "tags": _tags(1 + mix(n, 8) % 3, mix(n, 9)),
    }


def post(m: int) -> dict[str, object]:
    """The ``m``th post, counting from 0. It gets the key ``m + 1``."""
    return {
        "author": 1 + mix(m, 1) % PEOPLE,
        "title": f"post {m}",
        "body": " ".join(WORDS[mix(m, 100 + at) % 64] for at in range(12)),
        "likes": mix(m, 3) % 1_000,
        "language": _pick(LANGUAGES, m, 4),
        # A permutation of the posts, so that no two are made at once.
        "created_at": 1_650_000_000 + (m * 104_729 % POSTS) * 60,
        "pinned": mix(m, 7) % 50 == 0,
        "tags": _tags(mix(m, 5) % 3, mix(m, 6)),
    }


class Runs:
    """The parameters of each row's runs, counting runs from 0."""

    @staticmethod
    def detail(r: int) -> int:
        return 1 + mix(r, 11) % PEOPLE

    @staticmethod
    def filter(r: int) -> tuple[int, str]:
        return mix(r, 12) % 70, COUNTRIES[mix(r, 13) % 20]

    @staticmethod
    def either(r: int) -> tuple[str, str]:
        return COUNTRIES[r % 20], COUNTRIES[(r + 7) % 20]

    @staticmethod
    def prefix(r: int) -> str:
        return f"person {mix(r, 14) % 1_000}"

    @staticmethod
    def offset(r: int) -> int:
        return r * 997

    @staticmethod
    def author_country(r: int) -> str:
        return COUNTRIES[r % 20]

    @staticmethod
    def two_links(r: int) -> tuple[str, str]:
        return KINDS[r % 5], LANGUAGES[r % 6]

    @staticmethod
    def tag(r: int) -> str:
        return TAGS[r % 20]

    @staticmethod
    def phrase(r: int) -> str:
        return f"{WORDS[mix(r, 15) % 64]} {WORDS[mix(r, 16) % 64]}"

    @staticmethod
    def language(r: int) -> str:
        return LANGUAGES[r % 6]

    # The countries whose people turn a year older.
    UPDATED = ["kr", "us", "jp", "de"]

    @staticmethod
    def deleted(r: int) -> int:
        """The people deleted with their posts."""
        return 1 + r * 97

    DELETED = 1_000


def updated_count() -> int:
    """How many people ``update-query`` changes, which its time is divided by."""
    return sum(1 for n in range(PEOPLE) if _pick(COUNTRIES, n, 2) in Runs.UPDATED)


# The rows of the suite, with how many runs or objects each times.
LOAD = ("load", ORGANIZATIONS + PEOPLE + POSTS)
DETAIL = ("person-detail", 10_000)
FILTER = ("people-filter", 500)
EITHER = ("people-or", 100)
PREFIX = ("people-prefix", 100)
PAGE = ("people-page", 100)
AUTHOR_COUNTRY = ("posts-author-country", 100)
TWO_LINKS = ("posts-two-links", 30)
TAG = ("posts-tag", 200)
TEXT = ("posts-text", 10)
COUNT = ("posts-count", 200)
UPDATE = "update-query"
DELETE = "delete-cascade"


def batches(items: list[dict[str, object]]) -> list[list[dict[str, object]]]:
    """Splits ``items`` into lists of ``BATCH``."""
    return [items[at : at + BATCH] for at in range(0, len(items), BATCH)]
