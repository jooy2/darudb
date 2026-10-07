---
title: ReadTransaction
order: 5
---

# ReadTransaction

`ReadTransaction`은 자신을 시작한 `with db.read()` 블록이 도는 동안 데이터베이스의 커밋 하나를 읽습니다.

```python
class ReadTransaction: ...
```

`Database.read`가 돌려주는 컨텍스트 관리자는 블록이 시작할 때 읽기 트랜잭션을 시작하고, 블록에서 예외가 났든 안 났든 블록이 끝날 때 트랜잭션을 끝냅니다. 읽는 내용은 모두 트랜잭션을 시작할 때 게시돼 있던 커밋에서 나옵니다. 그사이 이 프로세스나 다른 프로세스가 커밋한 내용은 보이지 않습니다. 시작할 때 쓰기를 기다리지 않고, 쓰기도 이 트랜잭션을 기다리지 않습니다. 트랜잭션과 트랜잭션이 준 컬렉션은 블록 안에서만 쓸 수 있고, 그 뒤에 쓰면 모두 `CLOSED`를 일으킵니다.

다른 쪽이 쓰는 파일에서는 읽기 트랜잭션을 짧게 유지하세요. 읽기 트랜잭션이 열려 있는 동안에는 이후 커밋이 더는 쓰지 않는 페이지를 다시 쓸 수 없어서, 쓰기가 새 페이지를 잡고 파일이 커집니다. 자세한 설명은 [트랜잭션](../../guide/transactions.md)에 있습니다.

```python
from darudb import Query

with db.read() as txn:
    names = [user.name for user in txn.collection(User).find(Query().sort_by("name"))]
```

## 메서드

### collection

```python
@overload
def collection(self, collection: type[T]) -> ReadCollection[T]: ...
@overload
def collection(self, collection: str) -> ReadCollection[Any]: ...
```

클래스 `collection`의 컬렉션이나 그 이름의 컬렉션을 [ReadCollection](./read-collection.md)으로 돌려줍니다. 이 컬렉션의 객체는 클래스의 인스턴스입니다. 클래스로 가져오면 타입 검사기가 읽는 객체마다 타입을 압니다. 스키마에 없는 클래스나 이름이거나 스키마 없이 연 데이터베이스이면 `INVALID_ARGUMENT`를 일으킵니다. 이 핸들을 연 뒤에 다른 프로세스나 핸들이 파일을 마이그레이션했다면 컬렉션의 작업이 `SCHEMA_MISMATCH`로 실패하고, 새 스키마로 데이터베이스를 다시 열어야 합니다.

## AsyncReadTransaction

```python
class AsyncReadTransaction:
    @overload
    def collection(self, collection: type[T]) -> AsyncReadCollection[T]: ...
    @overload
    def collection(self, collection: str) -> AsyncReadCollection[Any]: ...
```

`async with db.read_async()`의 읽기 트랜잭션으로, 블록이 끝날 때까지 커밋 하나를 봅니다. `collection`은 위와 같이 await 없이 동작하되 [AsyncReadCollection](./read-collection.md#asyncreadcollection)을 돌려줍니다. 이 컬렉션의 작업은 코루틴이며, 패키지의 스레드 풀에서 시작한 순서대로 하나씩 실행됩니다. 트랜잭션은 블록이 끝나고 블록에서 시작한 작업도 모두 끝나야 마무리되며, 그 뒤에 시작한 작업은 `CLOSED`를 일으킵니다.

작업은 여느 코루틴처럼 await하거나 태스크가 실행할 때 실행됩니다. 한 번도 await하지 않은 작업은 실행되지 않습니다.

```python
import asyncio

from darudb import F


async def main() -> None:
    async with db.read_async() as txn:
        users = txn.collection(User)
        alice, adults = await asyncio.gather(users.get(1), users.count(F.age >= 18))
```
