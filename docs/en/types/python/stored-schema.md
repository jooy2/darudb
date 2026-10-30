---
title: StoredSchema
order: 13
group: objects
pageClass: reference-page
---

# StoredSchema

`StoredSchema` is a schema as the file stores it, by the names it stores, as [`Database.stored_schema`](../../api/python/database.md#stored-schema) gives it.

```python
@dataclass(frozen=True)
class StoredSchema:
    version: int
    collections: tuple[StoredCollection, ...]
```

It says what a [Schema](../../api/python/schema.md) of classes declares, by the names the file stores rather than the classes' attributes, without the ids the file adds: the version, and each collection with its fields, its primary key and its indexes. It describes the schema a database was opened with, or, for a database opened without one, the schema the file holds, which a program that opens a file it did not write learns its collections from. Every part of it is a frozen dataclass, and two equal schemas compare equal.

```python
with darudb.Database.open("app.darudb") as db:
    for collection in db.stored_schema.collections if db.stored_schema else ():
        for field in collection.fields:
            print(f"{collection.name}.{field.name}", field.type.kind, field.optional)
```

## StoredCollection

```python
@dataclass(frozen=True)
class StoredCollection:
    name: str
    key: str
    auto_increment: bool
    fields: tuple[StoredField, ...]
    indexes: tuple[StoredIndex, ...]
```

A collection. `key` names the primary key's field, `"id"` for the auto-increment, and `auto_increment` says whether the key is the auto-increment, an int `id` the engine assigns. `fields` are in the file's order, and the primary key's field is one of them unless the key is the auto-increment, whose `id` is not declared.

## StoredField

```python
@dataclass(frozen=True)
class StoredField:
    name: str
    type: StoredType
    optional: bool
    default: object = None
```

A field of a collection or of an embedded object, by the name the file stores. `optional` says whether it may be None, which it is when an object leaves it out. `default` is the value an object that leaves a required field out holds, and None when the field has none.

## StoredIndex

```python
@dataclass(frozen=True)
class StoredIndex:
    field: str
    unique: bool
    fields: tuple[str, ...] = ()
```

An index of a collection: the fields it is on, in order, each a stored name or a path such as `"address.city"`, and whether two objects may not hold the same values in all of them. `field` is the first of `fields`, the only one of an index declared with `field(index=True)` or `field(unique=True)`. A `StoredIndex` made without `fields` gets `(field,)`.

## StoredType

```python
@dataclass(frozen=True)
class StoredType:
    kind: Literal["bool", "int", "float", "string", "bytes", "link", "list", "object"]
    collection: str | None = None
    element: StoredType | None = None
    fields: tuple[StoredField, ...] = ()
```

A field's type, which `kind` names. A link names the `collection` whose primary key it holds, a list the type of its `element`, a scalar type or a link, and an embedded object its `fields`.
