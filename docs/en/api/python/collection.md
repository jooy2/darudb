---
title: collection
order: 2
counterpart: [/api/rust/derive, /api/dart/annotations]
---

# collection

`collection` and `embedded` make a class the objects of a collection or an embedded object, and `field` adds to a field what its annotation cannot say.

```python
@overload
def collection(cls: type[T], /) -> type[T]: ...
@overload
def collection(name: str | None = None, /) -> Callable[[type[T]], type[T]]: ...

def embedded(cls: type[T], /) -> type[T]: ...

def field(
    *,
    default: Any = MISSING,
    default_factory: Any = MISSING,
    primary_key: bool = False,
    index: bool = False,
    unique: bool = False,
    link: type | str | None = None,
    name: str | None = None,
) -> Any: ...
```

A collection is a class, and its annotations are its fields' types. A [Schema](./schema.md) lists the classes of a database, and a transaction reads and writes instances of them.

```python
import darudb
from darudb import field


@darudb.embedded
class Address:
    city: str
    zip: str | None = field(default=None, name="postcode")


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    tags: list[str] = field(default_factory=list, index=True)
    avatar: bytes | None = None
    address: Address | None = None


@darudb.collection("posts")
class Post:
    slug: str = field(primary_key=True)
    author: int = field(link=User, index=True)
    readers: list[int] = field(default_factory=list, link=User)


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User, Post]))
```

## The class

The decorator makes the class a frozen, keyword-only dataclass, as `dataclasses.dataclass(frozen=True, kw_only=True)` would, unless it is a dataclass already, which it keeps as it is. So an object is made with keywords, `User(name="Alice")`, a field with a default may come before one without, and an object is changed with `dataclasses.replace`. The decorators are declared with `dataclass_transform`, so type checkers know the constructor too.

- **Its fields are the dataclass's fields**, in the order the class declares them, and each annotation is the field's type in the file: `bool`, `int`, `float`, `str`, `bytes`, a `list` of those, or a class decorated with `@embedded`. [Field types](../../types/python/field-types.md) has the whole list.
- **`X | None` means optional.** The field may be `None`, and is `None` when it is left out. Its default is `None` or nothing at all, and any other default fails with `INVALID_ARGUMENT`. Without the default `None`, the file does not need the field, but the constructor does.
- **A default** is the value a field left out holds, both when an object is made and when a record written before the field existed is read. It is a constant the file can store: `default_factory` is called once, when the schema is made, for the value the file stores. A list needs `default_factory=list`, since a dataclass refuses a list as a default. An embedded object cannot be a default, and a default of another type than the field's fails with `INVALID_ARGUMENT` when the database opens.
- **A field without a default, and not optional, is required.**
- **The class has a `__dict__`.** A class whose instances have none, such as a dataclass made with `slots=True`, fails with `INVALID_ARGUMENT`, since an object read is built by filling its `__dict__`.

The classes are read when a [Schema](./schema.md) is made rather than when they are decorated, so an annotation may name a class declared after it. A class that breaks one of these rules fails then, with `INVALID_ARGUMENT` and a message that names the class and the field, and so does an annotation naming a type that does not exist.

## Objects read are built without `__init__`

An object read from the file is made with `object.__new__`, and its fields are put in its `__dict__`, so the class's `__init__` and `__post_init__` do not run for it. Calling `__init__` would cost a call and an attribute assignment per field for every object read. The object is an instance of the class, frozen if the class is, and equal to the same object made with the constructor, but a check in `__post_init__`, or an attribute that it sets, does not reach objects read.

## Primary keys

Every object has a primary key, which `get`, `update` and `delete` take and which no two objects of a collection share.

- **A key field.** A field marked `field(primary_key=True)` and annotated `int`, `str` or `bytes` is the key. It is required, has no default and is not optional, and a class has one at most; anything else fails with `INVALID_ARGUMENT`. An object's key never changes: `update` refuses a new one, and `put` under another key writes another object.
- **The auto-increment `id`.** A collection without a key field is keyed by a number the engine assigns, and the class needs the field `id: int | None = None`, under that name. It is `None` in an object that has not been inserted, and `insert` or `put` gives such an object the next number, from 1 up. An object inserted with an `id` of its own keeps it, and the numbers given later are greater than it. A class with neither a key field nor `id` fails with `INVALID_ARGUMENT`, with a message naming the field it needs.
- **Length.** A string or bytes key, and an indexed value, has to fit in a key of the file: at most 957 bytes once encoded, with 4096-byte pages, which is a few bytes more than the value. A longer one fails with `INVALID_ARGUMENT` when the object is written.

```python
with db.write() as txn:
    users = txn.collection(User)

    users.insert(User(name="Alice"))  # 1
    users.insert(User(id=10, name="Bob"))  # 10
    users.insert(User(name="Carol"))  # 11
    txn.collection(Post).insert(Post(slug="hello", author=1))  # "hello"
```

## @collection

```python
@overload
def collection(cls: type[T], /) -> type[T]: ...
@overload
def collection(name: str | None = None, /) -> Callable[[type[T]], type[T]]: ...
```

Makes the class the objects of a collection named `name`, or named as the class is: `@darudb.collection("users")`, or `@darudb.collection` and `@darudb.collection()` for a collection called `User`. The name is what the file stores and the query language, [Migration](./migration.md) and `txn.collection("users")` use. A name that is not a `str`, or a decorated value that is not a class, fails with `INVALID_ARGUMENT`.

## @embedded

```python
def embedded(cls: type[T], /) -> type[T]: ...
```

Makes the class an embedded object, which a field of another object holds whole. It is written without parentheses, `@darudb.embedded`, and a decorated value that is not a class fails with `INVALID_ARGUMENT`. An embedded object has no key and no collection, and none of its fields can be a primary key, indexed or unique, which fails with `INVALID_ARGUMENT`. An embedded class may hold another, but a list cannot hold embedded objects. A query reaches its fields with a path such as `F.address.city`.

## field

```python
def field(
    *,
    default: Any = MISSING,
    default_factory: Any = MISSING,
    primary_key: bool = False,
    index: bool = False,
    unique: bool = False,
    link: type | str | None = None,
    name: str | None = None,
) -> Any: ...
```

A field with what its annotation cannot say, as the value of the class attribute: `age: int = field(default=0, index=True)`. It returns a `dataclasses.field`, so it keeps the dataclass rules, such as not taking both `default` and `default_factory`.

### default and default_factory

The value of the field when it is left out, as `= value` would give it. `default_factory` is a function called for every object made, and once for the default the file stores.

### primary_key

The field is the collection's primary key, as [Primary keys](#primary-keys) describes.

### index

The engine keeps an index on the field, so that a query with a condition on it reads only the objects it finds, and a query sorted by it alone reads them in that order. An index on a list has an entry for each element.

### unique

An index that also refuses a second object with the same value, with `DUPLICATE_KEY`. Any number of objects may hold `None`. `unique=True` needs no `index=True` beside it.

### link

The field holds the primary key of an object of another collection, or of its own, given as the collection's class or its name; a collection links to itself by its name, since the class does not exist yet inside its own body. The annotation is the key's type, `int`, `str` or `bytes`, `| None` for an optional link, or a list of the key's type for a to-many link. A class that is not decorated with `@collection` fails with `INVALID_ARGUMENT` when the schema is made, and a collection the schema does not have when the database opens. A link to an object that does not exist is allowed. A query reads the linked object's fields through the link, as `F.author.name`.

### name

The name the file stores the field under, when it is not the attribute's. Queries built with [F](./conditions.md) and the changes of `update` name the attribute, and the package gives the engine the stored name. Text in the query language, a [Migration](./migration.md) and [Migrating.previous](./migrating.md#previous) use the stored name.
