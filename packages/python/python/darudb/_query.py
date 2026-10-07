"""Queries: conditions on fields, combined and sorted, and prepared queries.

``F`` names a field: ``F.age``, ``F.address.city`` through an embedded
object or a link, or ``F["name"]`` for a name that is also a method's.
Comparing a field makes a ``Condition``, and conditions combine with ``&``,
``|`` and ``~``. A ``Query`` adds a sort, an offset and a limit, and every
method returns a new query, so one can be kept and reused: it is compiled
once for each collection it runs on.

A path names a field by its Python attribute, and the package gives the
engine the name the file stores. A query in the query language names fields
as the file stores them.
"""

from __future__ import annotations

import threading
from collections import OrderedDict
from collections.abc import Iterable
from typing import TYPE_CHECKING, Any, Final, Generic, NoReturn, TypeVar

from . import _native
from ._errors import DaruError
from ._schema import resolve

if TYPE_CHECKING:
    from ._schema import Resolved, Schema

__all__ = ["Condition", "F", "FieldRef", "Param", "Prepared", "Query", "param", "where"]

T = TypeVar("T")

Param = _native.Param

_OPS: Final = {
    "==": 4,
    "!=": 5,
    "<": 6,
    "<=": 7,
    ">": 8,
    ">=": 9,
    "between": 10,
    "in": 11,
    "contains": 12,
    "startswith": 13,
    "endswith": 14,
}
_IS_NULL: Final = 15
_AND: Final = 1
_OR: Final = 2
_NOT: Final = 3


def param(index: int) -> Param:
    """A parameter in place of a value, in a query that ``Database.prepare``
    prepares: each run gives its value. ``param(0)`` is the first."""
    return _native.Param(index)


def _invalid_query(message: str) -> DaruError:
    return DaruError("INVALID_QUERY", message)


class Condition:
    """A test of one field, or tests combined with ``&``, ``|`` and ``~``."""

    __slots__ = ("_node", "_query")

    def __init__(self, node: tuple[Any, ...]) -> None:
        self._node = node
        self._query: Query | None = None

    def __and__(self, other: Condition) -> Condition:
        return _combine(_AND, self, other)

    def __or__(self, other: Condition) -> Condition:
        return _combine(_OR, self, other)

    def __invert__(self) -> Condition:
        return Condition((_NOT, self._node))

    def __bool__(self) -> NoReturn:
        raise TypeError("a condition has no truth value: combine conditions with &, | and ~")

    def __repr__(self) -> str:
        return f"Condition({self._node!r})"

    def query(self) -> Query:
        """A query with this condition alone, made once."""
        if self._query is None:
            self._query = Query()._with(filter=self._node)

        return self._query


def _combine(kind: int, left: Condition, right: Condition) -> Condition:
    if not isinstance(right, Condition):
        return NotImplemented

    terms: list[Any] = []

    for node in (left._node, right._node):
        # An AND inside an AND, and an OR inside an OR, flatten.
        if node[0] == kind:
            terms.extend(node[1])
        else:
            terms.append(node)

    return Condition((kind, tuple(terms)))


def _test(op: str, path: tuple[str, ...], values: tuple[Any, ...]) -> Condition:
    if op in ("==", "!=") and values[0] is None:
        is_null = (0, _IS_NULL, path, ())

        return Condition(is_null if op == "==" else (_NOT, is_null))

    for value in values:
        if value is None:
            raise _invalid_query(
                f"{'.'.join(path)} {op} compares with None, which only == and != do"
            )

    return Condition((0, _OPS[op], path, values))


class FieldRef:
    """A field of the objects a query tests, by its path."""

    __slots__ = ("_path",)

    __hash__ = None  # type: ignore[assignment]

    def __init__(self, path: tuple[str, ...]) -> None:
        self._path = path

    def __getattr__(self, name: str) -> FieldRef:
        if name.startswith("__"):
            raise AttributeError(name)

        return FieldRef((*self._path, name))

    def __getitem__(self, name: str) -> FieldRef:
        return FieldRef((*self._path, name))

    def __repr__(self) -> str:
        return f"F.{'.'.join(self._path)}"

    def __eq__(self, value: object) -> Condition:  # type: ignore[override]
        return _test("==", self._path, (value,))

    def __ne__(self, value: object) -> Condition:  # type: ignore[override]
        return _test("!=", self._path, (value,))

    def __lt__(self, value: object) -> Condition:
        return _test("<", self._path, (value,))

    def __le__(self, value: object) -> Condition:
        return _test("<=", self._path, (value,))

    def __gt__(self, value: object) -> Condition:
        return _test(">", self._path, (value,))

    def __ge__(self, value: object) -> Condition:
        return _test(">=", self._path, (value,))

    def between(self, low: object, high: object) -> Condition:
        """The field is from ``low`` to ``high``, both included."""
        return _test("between", self._path, (low, high))

    def is_in(self, values: Iterable[object]) -> Condition:
        """The field is one of ``values``."""
        return _test("in", self._path, tuple(values))

    def contains(self, value: object) -> Condition:
        """A string field contains ``value``, or a list holds the element ``value``."""
        return _test("contains", self._path, (value,))

    def startswith(self, value: object) -> Condition:
        """A string field starts with ``value``."""
        return _test("startswith", self._path, (value,))

    def endswith(self, value: object) -> Condition:
        """A string field ends with ``value``."""
        return _test("endswith", self._path, (value,))

    def is_null(self) -> Condition:
        """The field is None."""
        return Condition((0, _IS_NULL, self._path, ()))

    def is_not_null(self) -> Condition:
        """The field is not None."""
        return Condition((_NOT, (0, _IS_NULL, self._path, ())))


class _Fields:
    """``F``: the root of every field's path."""

    __slots__ = ()

    def __getattr__(self, name: str) -> FieldRef:
        if name.startswith("__"):
            raise AttributeError(name)

        return FieldRef((name,))

    def __getitem__(self, name: str) -> FieldRef:
        return FieldRef((name,))

    def __repr__(self) -> str:
        return "F"


F: Final = _Fields()
"""The fields of the objects a query tests: ``F.age >= 18``."""


def _path_of(field: str | FieldRef) -> tuple[str, ...]:
    if isinstance(field, FieldRef):
        return field._path

    if isinstance(field, str):
        return tuple(field.split("."))

    raise _invalid_query(f"a query sorts by a field, not {field!r}")


class Query:
    """What to find, in what order, and how many.

    Without a sort, objects come in primary key order, and objects that sort
    equal come in primary key order too. Each method returns a new query.
    """

    __slots__ = ("_compiled", "_filter", "_limit", "_offset", "_sort")

    def __init__(self) -> None:
        self._filter: tuple[Any, ...] | None = None
        self._sort: tuple[tuple[tuple[str, ...], bool], ...] = ()
        self._offset = 0
        self._limit: int | None = None
        self._compiled: dict[str, _native.NativeQuery] = {}

    def _with(self, **changes: Any) -> Query:
        query = Query()
        query._filter = changes.get("filter", self._filter)
        query._sort = changes.get("sort", self._sort)
        query._offset = changes.get("offset", self._offset)
        query._limit = changes.get("limit", self._limit)

        return query

    def where(self, condition: Condition) -> Query:
        """Adds ``condition``, which an object has to meet as well as any before."""
        if not isinstance(condition, Condition):
            raise _invalid_query(f"where takes a condition, not {condition!r}")

        if self._filter is None:
            return self._with(filter=condition._node)

        return self._with(filter=_combine(_AND, Condition(self._filter), condition)._node)

    def sort_by(self, field: str | FieldRef, *, descending: bool = False) -> Query:
        """Sorts by ``field``, ascending unless ``descending``, after any sort before."""
        return self._with(sort=(*self._sort, (_path_of(field), descending)))

    def offset(self, count: int) -> Query:
        """Skips the first ``count`` objects."""
        if count < 0:
            raise _invalid_query("an offset is not negative")

        return self._with(offset=count)

    def limit(self, count: int) -> Query:
        """Returns at most ``count`` objects."""
        if count < 0:
            raise _invalid_query("a limit is not negative")

        return self._with(limit=count)

    def __repr__(self) -> str:
        return (
            f"Query(filter={self._filter!r}, sort={self._sort!r}, "
            f"offset={self._offset}, limit={self._limit})"
        )

    def compile(self, resolved: Resolved, schema: Schema | None) -> _native.NativeQuery:
        """The query on ``resolved``'s collection, compiled once."""
        compiled = self._compiled.get(resolved.name)

        if compiled is None:
            translate = _Translator(resolved, schema)
            compiled = _native.compile_query(
                resolved.name,
                None if self._filter is None else translate.node(self._filter),
                [(translate.path(path), descending) for path, descending in self._sort],
                self._offset,
                self._limit,
            )
            self._compiled[resolved.name] = compiled

        return compiled


def where(condition: Condition) -> Query:
    """A query with ``condition``: ``where(F.age >= 18).sort_by(F.age)``."""
    return Query().where(condition)


class _Translator:
    """Turns the Python attributes of a path into the names the file stores,
    through embedded objects and links. A name it does not know passes as it
    is, for the engine to accept or refuse."""

    __slots__ = ("_resolved", "_schema")

    def __init__(self, resolved: Resolved, schema: Schema | None) -> None:
        self._resolved = resolved
        self._schema = schema

    def path(self, path: tuple[str, ...]) -> list[str]:
        names: list[str] = []
        current: Resolved | None = self._resolved

        for attr in path:
            if current is None:
                names.append(attr)
                continue

            found = next((item for item in current.fields if item.attr == attr), None)

            if found is None:
                names.append(attr)
                current = None
                continue

            names.append(found.stored)

            if found.embedded is not None:
                current = resolve(found.embedded)
            elif found.link is not None and self._schema is not None:
                try:
                    current = self._schema.resolved(found.link)
                except DaruError:
                    current = None
            else:
                current = None

        return names

    def node(self, node: tuple[Any, ...]) -> tuple[Any, ...]:
        if node[0] == 0:
            return (0, node[1], self.path(node[2]), node[3])

        if node[0] == _NOT:
            return (_NOT, self.node(node[1]))

        return (node[0], tuple(self.node(term) for term in node[1]))


_TEXT_CACHE: Final = 256
_parsed: OrderedDict[str, _native.NativeQuery] = OrderedDict()
# Threads of a free-threaded interpreter share the cache without a GIL.
_parsed_lock = threading.Lock()


def parse(text: str) -> _native.NativeQuery:
    """A query in the query language, parsed once for the last few hundred
    texts: a program tends to run the same few texts again and again."""
    with _parsed_lock:
        found = _parsed.get(text)

        if found is not None:
            _parsed.move_to_end(text)
            return found

    query = _native.parse_query(text)

    with _parsed_lock:
        _parsed[text] = query

        if len(_parsed) > _TEXT_CACHE:
            _parsed.popitem(last=False)

    return query


class Prepared(Generic[T]):
    """A query compiled once, on one collection, that each run gives values
    for its parameters. ``Database.prepare`` makes one. It holds no database
    or transaction, so it runs in any."""

    __slots__ = ("_native", "collection")

    collection: str
    """The collection the query runs on."""

    def __init__(self, collection: str, native: _native.NativeQuery) -> None:
        self.collection = collection
        self._native = native

    def __repr__(self) -> str:
        return f"Prepared({self.collection!r})"
