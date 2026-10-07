"""Declaring collections as classes, and the schema and migrations they make.

A class decorated with ``collection`` or ``embedded`` becomes a frozen,
keyword-only dataclass, and its annotations become its fields' types:
``bool``, ``int``, ``float``, ``str``, ``bytes``, a ``list`` of those, a class
decorated with ``embedded``, and any of them ``| None`` for an optional
field. ``field`` adds what an annotation cannot say: an index, a primary
key, a link, or a stored name of its own.

The classes are read when a ``Schema`` is made rather than when they are
decorated, so a class may name one declared after it.
"""

from __future__ import annotations

import dataclasses
import threading
import types
import typing
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import MISSING
from typing import Any, Final, Literal, TypeVar, dataclass_transform, overload

from . import _native
from ._errors import invalid

if typing.TYPE_CHECKING:
    from ._async import AsyncMigrating
    from ._database import Migrating

__all__ = ["Migration", "Schema", "collection", "embedded", "field"]

T = TypeVar("T")

_INFO: Final = "__darudb__"
_OPTIONS: Final = "darudb"


@dataclasses.dataclass(frozen=True)
class _Info:
    """What the decorator records on a class."""

    kind: Literal["collection", "embedded"]
    name: str


@dataclasses.dataclass(frozen=True)
class _Options:
    """What ``field`` adds to a dataclass field."""

    primary_key: bool = False
    index: bool = False
    unique: bool = False
    link: type | str | None = None
    name: str | None = None


@overload
def field(
    *,
    default: T,
    primary_key: bool = False,
    index: bool = False,
    unique: bool = False,
    link: type | str | None = None,
    name: str | None = None,
) -> T: ...


@overload
def field(
    *,
    default_factory: Callable[[], T],
    primary_key: bool = False,
    index: bool = False,
    unique: bool = False,
    link: type | str | None = None,
    name: str | None = None,
) -> T: ...


@overload
def field(
    *,
    primary_key: bool = False,
    index: bool = False,
    unique: bool = False,
    link: type | str | None = None,
    name: str | None = None,
) -> Any: ...


def field(
    *,
    default: Any = MISSING,
    default_factory: Any = MISSING,
    primary_key: bool = False,
    index: bool = False,
    unique: bool = False,
    link: type | str | None = None,
    name: str | None = None,
) -> Any:
    """A field of a collection or an embedded object, with what its annotation
    cannot say.

    - ``default`` or ``default_factory``: the value of a field left out, both
      when an object is made and when the file holds an object written before
      the field existed. It is a constant the file can store.
    - ``primary_key``: the field is the collection's primary key, an ``int``,
      a ``str`` or ``bytes``, required and without a default.
    - ``index``: queries on the field read an index rather than every object.
    - ``unique``: an index that also refuses two objects with the same value.
    - ``link``: the field holds the primary key of an object of another
      collection, given as its class or its name.
    - ``name``: the name the file stores the field under, when it is not the
      attribute's.
    """
    options = _Options(primary_key, index, unique, link, name)

    return dataclasses.field(
        default=default,
        default_factory=default_factory,
        metadata={_OPTIONS: options},
    )


def _decorate(cls: type[T], info: _Info) -> type[T]:
    if "__dataclass_fields__" not in cls.__dict__:
        cls = dataclasses.dataclass(frozen=True, kw_only=True)(cls)

    setattr(cls, _INFO, info)

    return cls


@overload
def collection(cls: type[T], /) -> type[T]: ...


@overload
def collection(name: str | None = None, /) -> Callable[[type[T]], type[T]]: ...


@dataclass_transform(kw_only_default=True, frozen_default=True, field_specifiers=(field,))
def collection(arg: Any = None, /) -> Any:
    """Makes a class the objects of a collection, named ``name`` or after the
    class.

    Without a field that is the primary key, the collection is keyed by an
    auto-increment, and the class needs a field ``id: int | None = None``:
    ``None`` in an object not yet inserted, which the engine gives the next
    number.
    """
    if isinstance(arg, type):
        return _decorate(arg, _Info("collection", arg.__name__))

    if arg is not None and not isinstance(arg, str):
        raise invalid(f"@collection takes a collection's name, not {arg!r}")

    def decorate(cls: type[T]) -> type[T]:
        _check_class(cls, "collection")

        return _decorate(cls, _Info("collection", arg if arg is not None else cls.__name__))

    return decorate


@dataclass_transform(kw_only_default=True, frozen_default=True, field_specifiers=(field,))
def embedded(cls: type[T], /) -> type[T]:
    """Makes a class an embedded object, which a field of another object holds.
    An embedded object has no key, and none of its fields can be indexed."""
    _check_class(cls, "embedded")

    return _decorate(cls, _Info("embedded", cls.__name__))


def _check_class(cls: object, kind: str) -> None:
    if not isinstance(cls, type):
        raise invalid(f"@{kind} decorates a class, not {cls!r}")


def info_of(cls: type) -> _Info | None:
    """What the decorator recorded on ``cls``, or ``None``."""
    info = cls.__dict__.get(_INFO) if isinstance(cls, type) else None

    return info if isinstance(info, _Info) else None


# A field type as the engine's schema takes it, and a kind as the native
# layout converts it.
_SCALARS: Final[dict[type, tuple[str, str]]] = {
    bool: ("bool", "bool"),
    int: ("int", "int"),
    float: ("float", "float"),
    str: ("str", "string"),
    bytes: ("bytes", "bytes"),
}
_KEYS: Final = (int, str, bytes)


@dataclasses.dataclass(frozen=True)
class _Field:
    """One field of a class as the schema and the layout need it."""

    attr: str
    stored: str
    kind: Any
    spec: tuple[str, Any, str, Any, bool, bool]
    link: str | None
    """The collection a link or a list of links names."""
    embedded: type | None
    """The class of an embedded object the field holds."""


@dataclasses.dataclass(frozen=True)
class Resolved:
    """A class read for the schema: its fields, its layout and its key."""

    cls: type
    name: str
    fields: tuple[_Field, ...]
    layout: _native.Layout
    key: str | None
    """The stored name of the primary key, ``"id"`` for an auto-increment."""


_RESOLVED: dict[type, Resolved] = {}
# Resolving a class twice at once would only make two equal layouts, but the
# lock keeps one, so that every query compiled for the class finds the same.
_RESOLVING = threading.RLock()


def _split_optional(annotation: Any) -> tuple[bool, Any]:
    """Whether ``annotation`` is ``X | None``, and ``X``."""
    origin = typing.get_origin(annotation)

    if origin is typing.Union or origin is types.UnionType:
        args = [arg for arg in typing.get_args(annotation) if arg is not type(None)]

        if len(args) == len(typing.get_args(annotation)) - 1 and len(args) == 1:
            return True, args[0]

    return False, annotation


def _link_name(link: type | str, owner: type, attr: str) -> str:
    if isinstance(link, str):
        return link

    info = info_of(link)

    if info is None or info.kind != "collection":
        raise invalid(
            f"{owner.__name__}.{attr} links to {link!r}, which is not a collection's class"
        )

    return info.name


def _type_of(annotation: Any, options: _Options, owner: type, attr: str) -> tuple[Any, Any]:
    """The kind and the type spec of a field annotated ``annotation``."""
    where = f"{owner.__name__}.{attr}"

    if typing.get_origin(annotation) is list:
        (element,) = typing.get_args(annotation) or (Any,)
        optional, _ = _split_optional(element)

        if optional:
            raise invalid(f"{where} is a list, which holds no None")

        kind, spec = _type_of(element, options, owner, attr)

        if isinstance(spec, tuple) and spec[0] in ("list", "object"):
            raise invalid(f"{where} is a list of lists or of objects, which the engine refuses")

        return ("list", kind), ("list", spec)

    if options.link is not None:
        if annotation not in _KEYS:
            raise invalid(f"{where} is a link, which holds a primary key: an int, a str or bytes")

        return "key", ("link", _link_name(options.link, owner, attr))

    if annotation in _SCALARS:
        return _SCALARS[annotation]

    if isinstance(annotation, type):
        info = info_of(annotation)

        if info is not None and info.kind == "embedded":
            resolved = resolve(annotation)

            return resolved.layout, ("object", [item.spec for item in resolved.fields])

    raise invalid(
        f"{where} has the type {annotation!r}; a field holds a bool, an int, a float, a str, "
        "bytes, a list of those, or an @embedded class"
    )


def resolve(cls: type) -> Resolved:
    """``cls`` read for the schema, once."""
    found = _RESOLVED.get(cls)

    if found is not None:
        return found

    with _RESOLVING:
        return _resolve(cls)


def _resolve(cls: type) -> Resolved:
    if cls in _RESOLVED:
        return _RESOLVED[cls]

    info = info_of(cls)

    if info is None:
        raise invalid(f"{cls!r} is not a class decorated with @collection or @embedded")

    if cls.__dictoffset__ == 0:
        # An object read is built by filling its `__dict__`.
        raise invalid(f"{cls.__name__} has __slots__, so an object read could not be built")

    try:
        hints = typing.get_type_hints(cls)
    except NameError as error:
        raise invalid(f"{cls.__name__} names a type that does not exist: {error}") from None

    fields: list[_Field] = []
    layout: list[tuple[str, str, Any]] = []
    key: str | None = None
    has_id = False
    declares_key = any(
        item.metadata.get(_OPTIONS, _Options()).primary_key for item in dataclasses.fields(cls)
    )

    for item in dataclasses.fields(cls):
        options = item.metadata.get(_OPTIONS, _Options())
        stored = options.name or item.name
        optional, annotation = _split_optional(hints[item.name])
        has_default = item.default is not MISSING or item.default_factory is not MISSING
        where = f"{cls.__name__}.{item.name}"

        if info.kind == "embedded" and (options.primary_key or options.index or options.unique):
            raise invalid(f"{where} is a field of an embedded object, which has no key or index")

        if (
            info.kind == "collection"
            and not declares_key
            and item.name == "id"
            and stored == "id"
            and annotation is int
            and optional
        ):
            # The auto-increment key, which the engine declares itself.
            has_id = True
            layout.append((item.name, stored, "int"))
            continue

        kind, spec = _type_of(annotation, options, cls, item.name)

        if options.primary_key:
            if optional or has_default:
                raise invalid(f"{where} is the primary key, which is required and has no default")

            if annotation not in _KEYS:
                raise invalid(f"{where} is the primary key, which is an int, a str or bytes")

            if key is not None:
                raise invalid(f"{cls.__name__} has two primary keys")

            key = stored
            mode, default = "key", None
        elif optional:
            if item.default is not MISSING and item.default is not None:
                raise invalid(f"{where} is optional, so it is None when left out")

            mode, default = "optional", None
        elif has_default:
            factory = typing.cast(Callable[[], Any], item.default_factory)
            default = item.default if item.default is not MISSING else factory()
            mode = "default"
        else:
            mode, default = "required", None

        field_spec = (stored, spec, mode, default, options.index, options.unique)
        link = spec[1] if isinstance(spec, tuple) and spec[0] == "link" else None

        if isinstance(spec, tuple) and spec[0] == "list" and isinstance(spec[1], tuple):
            link = spec[1][1] if spec[1][0] == "link" else None

        embedded_class = annotation if isinstance(kind, _native.Layout) else None

        fields.append(_Field(item.name, stored, kind, field_spec, link, embedded_class))
        layout.append((item.name, stored, kind))

    if info.kind == "collection" and key is None:
        if not has_id:
            raise invalid(
                f"{cls.__name__} has no primary key, so it needs the field "
                "`id: int | None = None`, which the engine numbers"
            )

        key = "id"

    resolved = Resolved(cls, info.name, tuple(fields), _native.Layout(cls, layout), key)
    _RESOLVED[cls] = resolved

    return resolved


class Schema:
    """The collections of a database, at a version.

    Opening a file with a schema stores it, or checks that the file holds
    the same one, or migrates a file that holds an older version. A changed
    schema needs a new version.
    """

    __slots__ = ("_by_class", "_by_name", "_native", "collections", "version")

    version: int
    collections: tuple[type, ...]

    def __init__(self, version: int, collections: Sequence[type]) -> None:
        if type(version) is not int or not 1 <= version < 2**63:
            raise invalid(f"a schema's version is a whole number from 1 up, not {version!r}")

        self.version = version
        self.collections = tuple(collections)
        self._by_class: dict[type, Resolved] = {}
        self._by_name: dict[str, Resolved] = {}

        for cls in self.collections:
            info = info_of(cls)

            if info is None or info.kind != "collection":
                raise invalid(f"{cls!r} is not a class decorated with @collection")

            resolved = resolve(cls)

            if resolved.name in self._by_name:
                raise invalid(f"two classes are the collection {resolved.name!r}")

            self._by_class[cls] = resolved
            self._by_name[resolved.name] = resolved

        self._native = _native.NativeSchema(
            version,
            [
                (item.name, [field.spec for field in item.fields])
                for item in self._by_class.values()
            ],
        )

    def __repr__(self) -> str:
        names = ", ".join(cls.__name__ for cls in self.collections)

        return f"Schema({self.version}, [{names}])"

    def resolved(self, collection: type | str) -> Resolved:
        """The collection a class or a name stands for, in this schema."""
        found = (
            self._by_name.get(collection)
            if isinstance(collection, str)
            else self._by_class.get(collection)
        )

        if found is None:
            name = collection if isinstance(collection, str) else collection.__name__

            raise invalid(f"the schema has no collection {name}")

        return found


@dataclasses.dataclass(frozen=True)
class Migration:
    """What schema version ``version`` changes from the version before it,
    beyond what the engine works out itself.

    The engine adds new collections, fields and indexes and drops what the
    schema no longer has. A migration renames collections and fields, deletes
    collections with their objects, replaces fields whose type changes, and
    runs ``run`` in the migration's write transaction, after the renames.
    With ``Database.open_async``, ``run`` may be a coroutine function.
    """

    version: int
    rename_collections: Sequence[tuple[str, str]] = ()
    """Pairs of the old name and the new. The objects stay where they are."""
    rename_fields: Sequence[tuple[str, str, str]] = ()
    """The collection's name before the migration, the field's old name and the new."""
    delete_collections: Sequence[str] = ()
    """Collections that go, with their objects."""
    replace_fields: Sequence[tuple[str, str]] = ()
    """Fields replaced by a new field of the same name, as when a type changes."""
    run: Callable[[Migrating], None] | Callable[[AsyncMigrating], Awaitable[None]] | None = None
    """The function of the step, given the migration's transaction."""

    def __post_init__(self) -> None:
        if type(self.version) is not int or not 1 <= self.version < 2**63:
            raise invalid(f"a migration's version is a whole number, not {self.version!r}")

        for name, size in (
            ("rename_collections", 2),
            ("rename_fields", 3),
            ("replace_fields", 2),
        ):
            for entry in getattr(self, name):
                if (
                    not isinstance(entry, tuple | list)
                    or len(entry) != size
                    or not all(isinstance(part, str) for part in entry)
                ):
                    raise invalid(f"each of {name} is {size} names, not {entry!r}")

        if isinstance(self.delete_collections, str) or not all(
            isinstance(name, str) for name in self.delete_collections
        ):
            raise invalid(f"delete_collections is names, not {self.delete_collections!r}")

        if self.run is not None and not callable(self.run):
            raise invalid(f"run is a function, not {self.run!r}")

    def spec(
        self,
    ) -> tuple[
        int, list[tuple[str, str]], list[tuple[str, str, str]], list[str], list[tuple[str, str]]
    ]:
        return (
            self.version,
            [tuple(pair) for pair in self.rename_collections],  # type: ignore[misc]
            [tuple(triple) for triple in self.rename_fields],  # type: ignore[misc]
            list(self.delete_collections),
            [tuple(pair) for pair in self.replace_fields],  # type: ignore[misc]
        )
