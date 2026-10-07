---
title: WriteCollection
order: 9
counterpart: /api/rust/collection-writer
---

# WriteCollection

`WriteCollection`은 쓰기 트랜잭션 안에서 컬렉션 하나의 객체를 쓰고, `ReadCollection`처럼 읽기도 합니다.

```python
class WriteCollection(ReadCollection[T]): ...
```

쓰기 트랜잭션이나 [Migrating](./migrating.md)의 `collection`이 돌려줍니다. 읽기에는 트랜잭션 자신의 변경도 보입니다. 메서드는 트랜잭션의 블록 안에서만 부를 수 있고, 그 뒤에는 `CLOSED`를 일으킵니다.

쓰기는 먼저 객체를 엔진의 값으로 바꾸고 스키마에 맞춰 검사합니다.

- **`INVALID_ARGUMENT`**: 맞지 않는 객체입니다. 컬렉션 클래스의 인스턴스가 아닌 객체, 필드와 타입이 다른 값, 필수 필드의 `None`, 64비트를 넘는 `int`, `int` 필드의 `bool`, 목록 안의 `None`, 대상 컬렉션의 키와 타입이 다른 키를 담은 링크가 그런 경우입니다. 필드마다 받는 값은 [필드 타입](../../types/python/field-types.md)에 있습니다.
- **`DUPLICATE_KEY`**: 넣으려는 키가 이미 있거나, 고유 인덱스에 객체의 값 가운데 하나가 이미 있습니다.
- **거부된 쓰기는 아무것도 바꾸지 않으므로**, 블록에서 오류를 잡으면 이어서 진행하고 커밋해도 됩니다.

쓰기는 `int`, `str`, `bytes` 중 하나인 [Key](../../types/python/key.md)를 돌려줍니다. 엔진이 지키는 규칙은 [컬렉션과 객체](../../guide/objects.md)에 있습니다.

```python
with db.write() as txn:
    users = txn.collection(User)
    alice, bob = users.insert_many(
        [User(name="Alice", email="alice@example.com", age=31), User(name="Bob", tags=["new"])]
    )

    users.update(alice, age=32, email=None)
    users.put(User(id=2, name="Robert", age=18))
    users.delete(bob)
```

## 메서드

### insert

```python
def insert(self, obj: T) -> Key: ...
```

`obj`를 넣고 기본 키를 돌려줍니다. 자동 증가 키를 쓰는 컬렉션에서 `id`가 `None`인 객체는 다음 번호를 받고, `id`가 있는 객체는 그 번호로 들어갑니다.

### insert_many

```python
def insert_many(self, objects: Iterable[T]) -> list[Key]: ...
```

반복할 수 있는 값으로 받은 `objects`를 엔진 호출 한 번으로 넣고, 키를 순서대로 돌려줍니다. 반복할 수 없는 값을 주면 `INVALID_ARGUMENT`로 실패합니다. 객체는 넣기 전에 모두 먼저 변환하므로, 변환할 수 없는 객체가 하나라도 있으면 아무것도 쓰기 전에 묶음 전체를 거부합니다. 엔진이 거부한 객체가 있으면 그 오류로 묶음이 멈춥니다. 앞서 넣은 객체는 트랜잭션에 남고, 블록에서 예외가 나지 않는 한 그대로 커밋됩니다. 네이티브 모듈은 객체마다가 아니라 묶음 전체에 한 번만 GIL을 놓습니다.

### put

```python
def put(self, obj: T) -> Key: ...
```

`obj`를 넣거나, 키가 같은 객체가 있으면 바꾸고, 키를 돌려줍니다. 키가 이미 있어도 실패하지 않는다는 점을 빼면 `insert`와 같은 경우에 실패합니다. 저장된 객체를 통째로 바꿉니다. 자동 증가 키를 쓰는 컬렉션에서 `id`가 `None`인 객체는 다음 번호로 들어갑니다.

### put_many

```python
def put_many(self, objects: Iterable[T]) -> list[Key]: ...
```

객체마다 `put`을 하되 엔진은 한 번만 부르며, 묶음 규칙은 `insert_many`와 같습니다.

### update

```python
def update(self, key: Key, /, **changes: object) -> bool: ...
```

기본 키가 `key`인 객체에서 `changes`가 속성 이름으로 가리킨 필드만 바꾸고, 객체가 있었는지 돌려줍니다. 객체가 없으면 아무것도 쓰지 않습니다. 나머지 필드는 그대로 둡니다. `key`는 위치 인자로만 받으므로 `key`라는 필드도 바꿀 수 있습니다.

- `None`을 주면 선택 필드는 `None`이 되고, 기본값이 있는 필드는 기본값이 됩니다. 기본값이 없는 필수 필드는 `None`으로 바꿀 수 없습니다.
- 내장 객체와 목록은 `update(1, address=Address(city="Seoul"))`처럼 통째로 바뀝니다.
- `field(name=...)`로 선언한 필드는 속성 이름으로 가리키고, 패키지가 엔진에는 저장된 이름을 넘깁니다.

거부되는 경우는 `put`과 같습니다. 클래스에 없는 속성이나 타입이 다른 값은 `INVALID_ARGUMENT`로, 다른 객체가 이미 쓰는 고유 값은 `DUPLICATE_KEY`로 실패합니다. 기본 키를 바꾸려 하면 `INVALID_ARGUMENT`로 실패하지만, 객체의 지금 키를 그대로 주는 것은 괜찮습니다. 바뀐 필드만 변환해서 엔진으로 넘깁니다.

```python
with db.write() as txn:
    txn.collection(User).update(1, age=37, email=None, tags=["admin"])
```

### delete

```python
def delete(self, key: Key) -> bool: ...
```

기본 키가 `key`인 객체를 인덱스 항목과 함께 지우고, 객체가 있었는지 돌려줍니다. `int`, `str`, 바이트가 아닌 키를 주면 `INVALID_ARGUMENT`로 실패합니다.

## AsyncWriteCollection

```python
class AsyncWriteCollection(AsyncReadCollection[T]):
    async def insert(self, obj: T) -> Key: ...
    async def insert_many(self, objects: Iterable[T]) -> list[Key]: ...
    async def put(self, obj: T) -> Key: ...
    async def put_many(self, objects: Iterable[T]) -> list[Key]: ...
    async def update(self, key: Key, /, **changes: object) -> bool: ...
    async def delete(self, key: Key) -> bool: ...
```

비동기 쓰기 트랜잭션의 컬렉션이며, [AsyncReadCollection](./read-collection.md#asyncreadcollection)의 읽기 메서드도 있습니다. 멤버가 하는 일은 위와 같되 코루틴입니다. 실패하면 작업을 await한 자리에서 같은 오류를 일으키고, 거부된 작업은 아무것도 바꾸지 않습니다. 작업은 시작한 순서대로 실행되고, 한 작업이 실패해도 그 뒤의 작업은 계속 실행됩니다. 작업마다 패키지의 스레드 풀을 한 번 오가므로, 객체 여러 개는 `insert_many`나 `put_many` 한 번에 넣으세요.

```python
import asyncio


async def main() -> None:
    async with db.write_async() as txn:
        users = txn.collection(User)

        await asyncio.gather(users.insert(User(name="Dave")), users.update(1, age=33), users.delete(3))
```
