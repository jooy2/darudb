---
title: WriteTransaction
order: 6
---

# WriteTransaction

`WriteTransaction`은 데이터베이스에 할 변경을 모아 두었다가, 자신을 시작한 `with db.write()` 블록이 끝날 때 한꺼번에 커밋합니다.

```python
class WriteTransaction(ReadTransaction): ...
```

`Database.write`가 돌려주는 컨텍스트 관리자는 블록이 시작할 때 쓰기 트랜잭션을 시작하고, 블록이 끝나면 커밋하고, 블록에서 예외가 나면 취소합니다. 그래서 예외가 난 블록이 한 일은 남지 않고, 예외는 그대로 밖으로 나갑니다. 트랜잭션은 시작할 때의 커밋에 자기 변경을 더한 상태를 읽습니다. `DUPLICATE_KEY`나 `INVALID_ARGUMENT`로 거부된 쓰기는 아무것도 바꾸지 않으므로, 블록에서 오류를 잡으면 이어서 진행하고 나머지를 커밋해도 됩니다. 트랜잭션과 그 컬렉션은 블록 안에서만 쓸 수 있고, 그 뒤에 쓰면 모두 `CLOSED`를 일으킵니다.

한 파일에서는 모든 스레드와 프로세스를 통틀어 한 번에 쓰기 트랜잭션 하나만 돕니다. 다른 스레드나 다른 프로세스가 쓰고 있으면 `busy_timeout`만큼, 기본값으로는 5초까지 기다리고, 그래도 쓰기가 끝나지 않으면 `BUSY`로 실패합니다. 한 스레드 안에서는 쓰기 트랜잭션이 겹칠 수 없습니다. 같은 파일에 연 쓰기 블록 안에서 `with db.write()`를 다시 쓰면 이 핸들로든 다른 핸들로든 자기 자신을 기다리는 대신 곧바로 `INVALID_ARGUMENT`로 실패합니다. 블록 안에서 같은 파일에 `sync`, `close`, `compact`, `set_key`, `set_password`를 불러도 마찬가지입니다. 이들도 이 블록을 기다리게 되기 때문입니다.

`db.write(durability="deferred")`로 지연 커밋을 요청하지 않았다면, 블록이 끝날 때 커밋은 디스크에 기록돼 있습니다. 지연 커밋은 디스크를 기다리지 않고 반환합니다. 커밋은 읽기에 곧바로 보이고, 프로세스가 비정상 종료돼도 잃지 않습니다. 디스크에는 다음 동기 커밋이나 `sync`, `close`를 부를 때 기록되고, 마지막 동기화 뒤의 지연 커밋이 1초를 기다렸거나 16,384페이지를 썼을 때도 기록됩니다. 두 가지 커밋은 [Durability](../../types/python/durability.md)에, 자세한 설명은 [트랜잭션](../../guide/transactions.md)에 있습니다.

```python
with db.write() as txn:
    users = txn.collection(User)
    key = users.insert(User(name="Alice", email="alice@example.com"))

    users.update(key, age=31)
```

## 메서드

### collection

```python
@overload
def collection(self, collection: type[T]) -> WriteCollection[T]: ...
@overload
def collection(self, collection: str) -> WriteCollection[Any]: ...
```

클래스 `collection`의 컬렉션이나 그 이름의 컬렉션을 [WriteCollection](./write-collection.md)으로 돌려줍니다. 이 컬렉션은 트랜잭션 자신의 변경까지 읽고, 변경을 더합니다. 스키마에 없는 클래스나 이름이거나 스키마 없이 연 데이터베이스이면 `INVALID_ARGUMENT`를 일으킵니다.

## AsyncWriteTransaction

```python
class AsyncWriteTransaction(AsyncReadTransaction):
    @overload
    def collection(self, collection: type[T]) -> AsyncWriteCollection[T]: ...
    @overload
    def collection(self, collection: str) -> AsyncWriteCollection[Any]: ...
```

`async with db.write_async()`의 쓰기 트랜잭션입니다. 이 이벤트 루프가 그 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤, 패키지의 스레드 풀에서 시작합니다. 블록이 끝나면 블록에서 시작한 작업이 모두 끝난 뒤에 커밋하고, 블록에서 예외가 나면 취소합니다. `collection`은 위와 같이 동작하되 [AsyncWriteCollection](./write-collection.md#asyncwritecollection)을 돌려줍니다. 이 컬렉션의 작업은 코루틴이며, 시작한 순서대로 풀에서 실행됩니다.

실패한 작업은 await한 자리에서 오류를 일으키고 아무것도 바꾸지 않습니다. 블록이 그 오류를 잡으면 트랜잭션은 나머지를 그대로 커밋합니다. 블록이 도는 동안 이벤트 루프의 스레드에서 같은 파일에 동기 `write`, `sync`, `close`, `compact`, `set_key`, `set_password`를 부르면 `INVALID_ARGUMENT`로 거부됩니다. 이 쓰기가 끝나려면 그 이벤트 루프가 필요하기 때문입니다. 블록 안이나 블록 안에서 만든 태스크에서 같은 파일에 `write_async`, `sync_async`, `close_async`, `compact_async`, `set_key_async`, `set_password_async`를 await해도 거부됩니다. 자기가 속한 쓰기를 기다리게 되기 때문입니다.

```python
import dataclasses

from darudb import F


async def main() -> None:
    async with db.write_async() as txn:
        users = txn.collection(User)
        bob = await users.find_one(F.name == "Bob")

        if bob is not None:
            await users.put(dataclasses.replace(bob, age=bob.age + 1))

        await users.insert(User(name="Carol"))
```
