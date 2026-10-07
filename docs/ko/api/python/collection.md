---
title: collection
order: 2
counterpart: [/api/rust/derive, /api/dart/annotations]
---

# collection

`collection`과 `embedded`는 클래스를 컬렉션의 객체나 내장 객체로 만들고, `field`는 어노테이션으로 나타낼 수 없는 것을 필드에 더합니다.

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

컬렉션은 클래스이고, 클래스의 어노테이션이 필드의 타입입니다. [Schema](./schema.md)는 데이터베이스의 클래스를 나열하고, 트랜잭션은 그 클래스의 인스턴스를 읽고 씁니다.

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

## 클래스

데코레이터는 클래스를 `dataclasses.dataclass(frozen=True, kw_only=True)`처럼 필드를 바꿀 수 없고 키워드 인자로만 만드는 데이터 클래스로 만듭니다. 이미 데이터 클래스이면 그대로 둡니다. 그래서 객체는 `User(name="Alice")`처럼 키워드로 만들고, 기본값이 있는 필드를 기본값이 없는 필드보다 앞에 둘 수 있으며, 객체를 바꿀 때는 `dataclasses.replace`를 씁니다. 데코레이터는 `dataclass_transform`으로 선언돼 있어서 타입 검사기도 생성자를 압니다.

- **필드는 데이터 클래스의 필드입니다.** 클래스가 선언한 순서를 따르고, 어노테이션이 파일에서의 필드 타입입니다. `bool`, `int`, `float`, `str`, `bytes`, 이들의 `list`, `@embedded`를 붙인 클래스를 쓸 수 있습니다. 전체 목록은 [필드 타입](../../types/python/field-types.md)에 있습니다.
- **`X | None`은 선택 필드입니다.** 필드는 `None`일 수 있고, 빠지면 `None`이 됩니다. 기본값은 `None`이거나 아예 없어야 하며, 다른 기본값은 `INVALID_ARGUMENT`로 실패합니다. 기본값 `None`이 없으면 파일에는 그 필드가 없어도 되지만, 생성자에는 넘겨야 합니다.
- **기본값**은 빠진 필드에 들어가는 값입니다. 객체를 만들 때도, 필드가 생기기 전에 쓴 레코드를 읽을 때도 이 값이 들어갑니다. 기본값은 파일에 저장할 수 있는 상수여서, 파일에 저장할 값을 얻으려고 스키마를 만들 때 `default_factory`를 한 번 부릅니다. 데이터 클래스는 목록을 기본값으로 받지 않으므로 목록 필드에는 `default_factory=list`를 씁니다. 내장 객체는 기본값이 될 수 없고, 필드와 타입이 다른 기본값은 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다.
- **기본값이 없고 선택 필드도 아닌 필드는 필수입니다.**
- **클래스에는 `__dict__`가 있어야 합니다.** 읽은 객체는 `__dict__`를 채워서 만들기 때문에, `slots=True`로 만든 데이터 클래스처럼 인스턴스에 `__dict__`가 없는 클래스는 `INVALID_ARGUMENT`로 실패합니다.

클래스는 데코레이터를 붙일 때가 아니라 [Schema](./schema.md)를 만들 때 읽으므로, 어노테이션에 뒤에서 선언한 클래스를 써도 됩니다. 이 규칙 중 하나라도 어긴 클래스는 그때 클래스와 필드 이름을 담은 메시지와 함께 `INVALID_ARGUMENT`로 실패하고, 없는 타입을 쓴 어노테이션도 마찬가지입니다.

## 읽은 객체는 `__init__`을 거치지 않습니다

파일에서 읽은 객체는 `object.__new__`로 만들고 필드를 `__dict__`에 넣으므로, 클래스의 `__init__`과 `__post_init__`은 실행되지 않습니다. `__init__`을 부르면 읽는 객체마다 호출 한 번과 필드마다 속성 대입 한 번이 더 듭니다. 읽은 객체는 그 클래스의 인스턴스이고, 클래스가 동결돼 있으면 객체도 동결되며, 생성자로 만든 같은 객체와 같다고 비교됩니다. 하지만 `__post_init__`에서 하는 검사나 거기서 정하는 속성은 읽은 객체에 적용되지 않습니다.

## 기본 키

모든 객체에는 기본 키가 있습니다. `get`, `update`, `delete`가 기본 키를 받고, 한 컬렉션의 두 객체가 같은 기본 키를 쓸 수는 없습니다.

- **키 필드.** `field(primary_key=True)`를 붙이고 `int`, `str`, `bytes` 중 하나로 어노테이션한 필드가 키입니다. 키 필드는 필수이고 기본값이 없으며 선택 필드가 될 수 없고, 클래스마다 하나까지입니다. 이를 어기면 `INVALID_ARGUMENT`로 실패합니다. 객체의 키는 바뀌지 않습니다. `update`는 새 키를 거부하고, 다른 키로 `put`하면 다른 객체를 씁니다.
- **자동 증가 `id`.** 키 필드가 없는 컬렉션은 엔진이 매기는 번호를 키로 쓰고, 클래스에는 바로 이 이름으로 `id: int | None = None` 필드가 있어야 합니다. 아직 넣지 않은 객체에서는 `None`이고, `insert`나 `put`이 그런 객체에 1부터 다음 번호를 줍니다. `id`를 직접 정해 넣은 객체는 그 번호를 그대로 쓰고, 그 뒤에 매기는 번호는 그보다 큽니다. 키 필드도 `id`도 없는 클래스는 필요한 필드를 알려 주는 메시지와 함께 `INVALID_ARGUMENT`로 실패합니다.
- **길이.** 문자열 키와 바이트 키, 인덱스에 들어가는 값은 파일의 키에 들어가야 합니다. 4096바이트 페이지에서는 인코딩한 길이가 957바이트까지이고, 인코딩하면 값보다 몇 바이트 길어집니다. 더 긴 값은 객체를 쓸 때 `INVALID_ARGUMENT`로 실패합니다.

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

클래스를 `name`이라는 컬렉션의 객체로 만듭니다. 이름을 주지 않으면 클래스 이름을 씁니다. `@darudb.collection("users")`처럼 쓰고, 클래스 `User`에 `@darudb.collection`이나 `@darudb.collection()`을 붙이면 컬렉션 이름은 `User`입니다. 이 이름은 파일에 저장되고, 쿼리 언어와 [Migration](./migration.md), `txn.collection("users")`가 이 이름을 씁니다. 이름이 `str`이 아니거나 데코레이터를 클래스가 아닌 값에 붙이면 `INVALID_ARGUMENT`로 실패합니다.

## @embedded

```python
def embedded(cls: type[T], /) -> type[T]: ...
```

클래스를 내장 객체로 만듭니다. 내장 객체는 다른 객체의 필드에 통째로 담깁니다. 괄호 없이 `@darudb.embedded`로 쓰고, 클래스가 아닌 값에 붙이면 `INVALID_ARGUMENT`로 실패합니다. 내장 객체에는 키도 컬렉션도 없고, 그 필드는 기본 키도 인덱스도 고유 필드도 될 수 없습니다. 이를 어기면 `INVALID_ARGUMENT`로 실패합니다. 내장 클래스 안에 다른 내장 클래스를 담을 수는 있지만, 목록에는 내장 객체를 담을 수 없습니다. 쿼리는 `F.address.city` 같은 경로로 내장 객체의 필드에 닿습니다.

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

어노테이션으로 나타낼 수 없는 것을 담은 필드로, `age: int = field(default=0, index=True)`처럼 클래스 속성의 값으로 씁니다. `dataclasses.field`를 돌려주므로 데이터 클래스의 규칙도 그대로 따릅니다. `default`와 `default_factory`를 함께 받지 않는 것이 그 예입니다.

### default와 default_factory

필드가 빠졌을 때의 값으로, `= value`로 쓴 것과 같습니다. `default_factory`는 객체를 만들 때마다 부르는 함수이고, 파일에 저장할 기본값을 얻을 때도 한 번 부릅니다.

### primary_key

필드를 컬렉션의 기본 키로 만듭니다. [기본 키](#기본-키)에서 설명합니다.

### index

엔진이 필드에 인덱스를 둡니다. 그 필드에 조건을 건 쿼리는 찾는 객체만 읽고, 그 필드 하나로만 정렬한 쿼리는 정렬 순서대로 읽습니다. 목록 필드의 인덱스에는 원소마다 항목이 생깁니다.

### unique

필드에 인덱스를 두되, 값이 같은 객체가 또 들어오면 `DUPLICATE_KEY`로 거부합니다. `None`은 몇 개가 있어도 됩니다. `unique=True`만 쓰면 되고 `index=True`를 함께 쓸 필요는 없습니다.

### link

필드에 다른 컬렉션이나 자기 컬렉션에 있는 객체의 기본 키를 담습니다. 컬렉션은 클래스나 이름으로 줍니다. 클래스 본문 안에서는 그 클래스가 아직 없으므로, 자기 컬렉션을 가리킬 때는 이름을 씁니다. 어노테이션은 키의 타입인 `int`, `str`, `bytes`이고, 선택 링크면 `| None`을 붙이며, 여러 객체를 가리키는 링크면 키 타입의 목록입니다. `@collection`을 붙이지 않은 클래스는 스키마를 만들 때, 스키마에 없는 컬렉션은 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다. 없는 객체를 가리키는 링크도 허용됩니다. 쿼리는 `F.author.name`처럼 링크를 지나 가리키는 객체의 필드를 읽습니다.

### name

파일에 저장할 필드 이름을 속성 이름 대신 정합니다. [F](./conditions.md)로 만든 쿼리와 `update`의 변경은 속성 이름으로 필드를 가리키고, 패키지가 엔진에는 저장된 이름을 넘깁니다. 쿼리 언어 문자열과 [Migration](./migration.md), [Migrating.previous](./migrating.md#previous)는 저장된 이름을 씁니다.
