---
title: StoredSchema
order: 13
group: objects
pageClass: reference-page
---

# StoredSchema

`StoredSchema`는 파일이 저장한 스키마를 저장된 이름 그대로 담은 것으로, [`Database.stored_schema`](../../api/python/database.md#stored-schema)가 돌려줍니다.

```python
@dataclass(frozen=True)
class StoredSchema:
    version: int
    collections: tuple[StoredCollection, ...]
```

클래스로 만든 [Schema](../../api/python/schema.md)가 선언하는 내용을, 클래스의 속성 이름이 아니라 파일에 저장된 이름으로 담습니다. 파일이 붙이는 ID는 없습니다. 버전과 컬렉션 목록이 있고, 컬렉션마다 필드와 기본 키, 인덱스가 있습니다. 데이터베이스를 열 때 준 스키마를 나타내고, 스키마 없이 열었으면 파일에 저장된 스키마를 나타냅니다. 직접 쓰지 않은 파일을 여는 프로그램은 여기서 컬렉션을 알아냅니다. 모든 부분이 얼린 데이터클래스이고, 내용이 같은 두 스키마는 같다고 비교됩니다.

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

컬렉션 하나입니다. `key`는 기본 키 필드의 이름이고, 자동 증가 키라면 `"id"`입니다. `auto_increment`는 기본 키가 엔진이 매기는 int `id`인지 알려 줍니다. `fields`는 파일에 저장된 순서입니다. 기본 키 필드도 여기에 들어 있지만, 자동 증가 키의 `id`는 선언한 필드가 아니므로 빠집니다.

## StoredField

```python
@dataclass(frozen=True)
class StoredField:
    name: str
    type: StoredType
    optional: bool
    default: object = None
```

컬렉션이나 내장 객체의 필드이며, 이름은 파일에 저장된 이름입니다. `optional`은 필드가 None일 수 있는지 알려 주며, 객체에서 빠지면 None이 됩니다. `default`는 필수 필드가 객체에서 빠졌을 때 들어가는 값이고, 기본값이 없으면 None입니다.

## StoredIndex

```python
@dataclass(frozen=True)
class StoredIndex:
    field: str
    unique: bool
    fields: tuple[str, ...] = ()
```

컬렉션의 인덱스입니다. 인덱스를 둔 필드들을 순서대로 담고, 각 필드는 저장된 이름이거나 `"address.city"` 같은 경로입니다. `unique`는 모든 필드의 값이 같은 두 객체를 거부하는 고유 인덱스인지 알려 줍니다. `field`는 `fields`의 첫 필드이며, `field(index=True)`나 `field(unique=True)`로 선언한 인덱스에는 이 필드 하나뿐입니다. `fields` 없이 만든 `StoredIndex`는 `(field,)`를 받습니다.

## StoredType

```python
@dataclass(frozen=True)
class StoredType:
    kind: Literal["bool", "int", "float", "string", "bytes", "link", "list", "object"]
    collection: str | None = None
    element: StoredType | None = None
    fields: tuple[StoredField, ...] = ()
```

필드의 타입이며, `kind`가 종류를 나타냅니다. 링크는 기본 키를 담는 대상 `collection`을, 목록은 원소의 타입인 `element`를, 내장 객체는 자기 `fields`를 함께 담습니다. 목록의 원소는 스칼라 타입이나 링크입니다.
