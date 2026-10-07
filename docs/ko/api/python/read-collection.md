---
title: ReadCollection
order: 8
counterpart: /api/rust/collection-reader
---

# ReadCollection

`ReadCollection`은 트랜잭션 안에서 컬렉션 하나의 객체를 기본 키나 쿼리로 읽습니다.

```python
class ReadCollection(Generic[T]): ...
```

읽기 트랜잭션의 `collection`이 돌려주며, 쓰기 트랜잭션의 [WriteCollection](./write-collection.md)에도 아래 멤버가 모두 있어서 트랜잭션 자신의 변경까지 읽습니다. `T`는 컬렉션의 클래스입니다. 메서드는 트랜잭션의 블록 안에서만 부를 수 있고, 그 뒤에는 `CLOSED`를 일으킵니다.

읽어 온 객체는 트랜잭션이 끝나도 남는 클래스의 인스턴스이고, [collection](./collection.md#읽은-객체는-init-을-거치지-않습니다)에서 설명하듯 `__init__` 없이 만듭니다. 필드가 생기기 전에 써서 레코드에 없는 필드에는 기본값이나 `None`이 들어가고, 자동 증가 키를 쓰는 컬렉션의 객체에는 `id`가 들어 있습니다. 필드마다 어떤 값으로 읽히는지는 [필드 타입](../../types/python/field-types.md)에 있습니다.

## 속성

### name

```python
@property
def name(self) -> str: ...
```

파일에 저장된 컬렉션의 이름입니다.

## 메서드

### get

```python
def get(self, key: Key) -> T | None: ...
```

기본 키가 `key`인 객체이고, 없으면 `None`입니다. [Key](../../types/python/key.md)는 컬렉션의 키 타입에 맞는 `int`, `str`, 바이트입니다. 타입이 다른 키나 `bool`, `float`, `None`을 주면 `INVALID_ARGUMENT`로 실패합니다.

### find

```python
def find(self, query: QueryInput = None, /, *parameters: object) -> list[T]: ...
```

쿼리가 찾은 객체를 쿼리의 순서대로 돌려줍니다. 쿼리는 [QueryInput](../../types/python/query-input.md)에 선언된 대로 다음 중 하나로 줍니다.

- **없음**: 모든 객체를 기본 키 순서로 돌려줍니다.
- **[Query](./query.md)**: `where(F.age >= 18).sort_by(F.age)`처럼 만든 쿼리입니다.
- **[조건](./conditions.md)**: `F.age >= 18`처럼 만들며, 그 조건 하나만 있는 쿼리로 칩니다.
- **문자열**: [쿼리 언어](./query.md#쿼리-언어)로 쓴 쿼리입니다. `$0`, `$1` 같은 매개변수에는 `parameters`의 값이 순서대로 들어갑니다. 패키지는 프로세스의 모든 데이터베이스를 통틀어 마지막으로 해석한 문자열 256개를 기억하므로, 같은 문자열을 다시 실행하면 해석을 건너뜁니다.
- **[Prepared](../../types/python/prepared.md) 쿼리**: `Database.prepare`로 이 컬렉션에 준비한 쿼리입니다.

`parameters`는 쿼리 뒤에 위치 인자로 줍니다. `find("age >= $0", 18)`처럼 씁니다. 값 자리에 [param](./param.md)을 넣어 만든 쿼리도 준비했든 안 했든 같은 방식으로 값을 받습니다. 매개변수가 없는 쿼리는 받은 값을 무시합니다.

쿼리가 맞지 않으면 `INVALID_QUERY`로 실패합니다. 컬렉션에 없는 필드, 타입이 다른 값, 해석할 수 없는 문자열, 다른 컬렉션에 준비한 쿼리, 값을 받지 못한 매개변수, 쿼리가 아닌 값이 그런 경우입니다.

```python
from darudb import F, Query, where

adults = where(F.age >= 18)

with db.read() as txn:
    users = txn.collection(User)

    users.find()
    users.find(F.tags.contains("new"))
    users.find(adults.sort_by(F.age, descending=True).limit(10))
    users.find(Query().sort_by(F.name))
    users.find("age >= $0 AND name STARTSWITH $1", 18, "A")
```

`F`로 만든 쿼리는 컬렉션의 클래스로 처음 실행할 때 컴파일되고, 쿼리는 클래스와 스키마마다 컴파일한 결과를 기억해 둡니다. 그래서 변수에 담아 둔 쿼리를 다시 실행하면 다시 컴파일하지 않습니다.

### find_one

```python
def find_one(self, query: QueryInput = None, /, *parameters: object) -> T | None: ...
```

쿼리가 찾은 첫 객체이고, 없으면 `None`입니다. 쿼리는 `find`와 같은 형태로 받으며, 엔진은 첫 객체에서 읽기를 멈춥니다. 쿼리에 준 오프셋과 개수 제한은 그대로 적용되므로, 개수 제한이 0이면 아무것도 찾지 않습니다.

### count

```python
def count(self, query: QueryInput = None, /, *parameters: object) -> int: ...
```

쿼리가 찾은 객체의 수입니다. 오프셋을 건너뛰고 개수 제한 안에서 셉니다. 쿼리를 주지 않으면 모든 객체를 세는데, 이때는 객체를 읽지 않고 컬렉션이 기록해 둔 개수만 읽습니다.

## AsyncReadCollection

```python
class AsyncReadCollection(Generic[T]):
    @property
    def name(self) -> str: ...
    async def get(self, key: Key) -> T | None: ...
    async def find(self, query: QueryInput = None, /, *parameters: object) -> list[T]: ...
    async def find_one(self, query: QueryInput = None, /, *parameters: object) -> T | None: ...
    async def count(self, query: QueryInput = None, /, *parameters: object) -> int: ...
```

비동기 트랜잭션의 컬렉션입니다. 멤버가 하는 일은 위와 같되 코루틴이고, `CLOSED`를 포함해 실패하면 작업을 await한 자리에서 같은 오류를 일으킵니다. 작업은 패키지의 스레드 풀에서 시작한 순서대로 하나씩 실행되고, 작업마다 풀을 따로 오갑니다.

```python
import asyncio


async def main() -> None:
    async with db.read_async() as txn:
        users = txn.collection(User)
        found = await asyncio.gather(*(users.get(key) for key in (1, 2, 3)))
```
