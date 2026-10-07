---
title: Migrating
order: 7
counterpart: /api/dart/migration-context
---

# Migrating

`Migrating`은 마이그레이션 함수가 실행되는 쓰기 트랜잭션으로, 새 스키마의 컬렉션과 함께 마이그레이션 전 스키마로 읽은 객체를 보여 줍니다.

```python
class Migrating(WriteTransaction): ...
```

`Database.open`은 파일의 스키마 버전이 더 낮으면 쓰기 트랜잭션 하나 안에서 파일을 마이그레이션합니다. 먼저 단계마다 적힌 이름 바꾸기와 엔진이 알아서 하는 변경을 적용하고, 두 버전 사이에 있는 [Migration](./migration.md)의 `run` 함수를 버전 순서대로 `Migrating`과 함께 부릅니다. `collection`은 [WriteTransaction](./write-transaction.md)처럼 새 스키마의 컬렉션을 클래스나 이름으로 돌려줍니다. 함수가 예외를 일으키면 마이그레이션에서 한 일은 하나도 남지 않고, 파일은 예전 스키마와 데이터를 그대로 유지하며, `open`도 같은 예외를 일으킵니다. 엔진이 알아서 하는 변경과 함수가 필요한 변경은 [마이그레이션](../../guide/migrations.md)에서 설명합니다.

트랜잭션과 그 컬렉션은 마이그레이션이 진행되는 동안에만 쓸 수 있고, 그 뒤에 쓰면 모두 `CLOSED`를 일으킵니다.

```python
import darudb
from darudb import Migration, Migrating


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    age: int = 0


def ages(m: Migrating) -> None:
    users = m.collection(User)

    for key in m.previous_keys("people"):
        before = m.previous("people", key)

        if before is not None:
            users.update(key, age=int(before["age"]))


db = darudb.Database.open(
    "app.darudb",
    schema=darudb.Schema(2, [User]),
    migrations=[
        Migration(
            2,
            rename_collections=[("people", "users")],
            rename_fields=[("people", "fullName", "name")],
            replace_fields=[("people", "age")],
            run=ages,
        )
    ],
)
```

`age`가 `int`가 된 [Migration](./migration.md)의 예제와 같습니다.

## 속성

### previous_version

```python
@property
def previous_version(self) -> int: ...
```

마이그레이션 전에 파일에 있던 스키마 버전입니다. 모든 단계에서 같습니다.

### version

```python
@property
def version(self) -> int: ...
```

이 단계가 마이그레이션해 가는 스키마 버전으로, 단계를 적은 [Migration](./migration.md)의 `version`입니다. 버전 1에서 3으로 가는 마이그레이션은 2로 가는 단계를 `version` 2로, 그다음 3으로 가는 단계를 `version` 3으로 실행합니다.

## 메서드

### previous

```python
def previous(self, collection: str, key: Key) -> dict[str, Any] | None: ...
```

`collection`에서 기본 키가 `key`인 객체를 마이그레이션 전 스키마대로 읽어 돌려주고, 없으면 `None`을 돌려줍니다. 컬렉션과 객체의 필드는 마이그레이션 전에 파일에서 쓰던 이름이고, 마이그레이션이 지우거나 교체한 필드의 값도 그대로 나옵니다. 객체는 그 이름을 키로 쓰는 `dict`입니다. 자동 증가 키를 쓰는 컬렉션의 키는 `id`에 들어 있고, 레코드에 없는 필드에는 기본값이나 `None`이 들어갑니다. 내장 객체는 따로 `dict`가 되고, 링크는 담긴 키가 되며, 목록은 `list`가 됩니다. 마이그레이션이 지우는 컬렉션도 커밋 전까지는 이렇게 읽을 수 있습니다. 예전 스키마에 없던 컬렉션이면 `INVALID_ARGUMENT`로 실패합니다.

객체는 지금 상태 그대로 읽히고, 객체를 쓰면 새 스키마의 필드만 남습니다. 그러니 객체를 쓰기 전에 이렇게 읽어 두세요. 예전 스키마의 클래스는 보통 프로그램에 남아 있지 않으므로 객체를 `dict`로 돌려줍니다.

### previous_keys

```python
def previous_keys(self, collection: str) -> list[Key]: ...
```

마이그레이션 전 이름으로 가리킨 `collection`에 있는 모든 객체의 기본 키를 키 순서대로 돌려줍니다. 키는 컬렉션의 키 타입에 따라 `int`, `str`, `bytes` 값입니다.

## AsyncMigrating

```python
class AsyncMigrating(AsyncWriteTransaction):
    @property
    def previous_version(self) -> int: ...
    @property
    def version(self) -> int: ...
    async def previous(self, collection: str, key: Key) -> dict[str, Any] | None: ...
    async def previous_keys(self, collection: str) -> list[Key]: ...
```

`Database.open_async`가 하는 마이그레이션의 쓰기 트랜잭션으로, 이때 `run`은 코루틴 함수여도 됩니다. `previous`와 `previous_keys`는 코루틴이고, `collection`은 [AsyncWriteCollection](./write-collection.md#asyncwritecollection)을 돌려줍니다. 작업은 시작한 순서대로 실행되고, 한 단계는 함수가 반환하고 코루틴까지 끝난 뒤, 함수가 시작한 작업이 모두 끝나야 마무리됩니다. 마이그레이션은 비동기 쓰기처럼 파일을 쥐고 있으므로, 함수 안에서 같은 파일에 `write_async`를 부르면 어느 핸들로든 마이그레이션을 기다리는 대신 `INVALID_ARGUMENT`로 실패합니다.

```python
from darudb import AsyncMigrating


async def ages(m: AsyncMigrating) -> None:
    users = m.collection(User)

    for key in await m.previous_keys("people"):
        before = await m.previous("people", key)

        if before is not None:
            await users.update(key, age=int(before["age"]))


async def main() -> None:
    db = await darudb.Database.open_async(
        "app.darudb",
        schema=darudb.Schema(2, [User]),
        migrations=[
            Migration(
                2,
                rename_collections=[("people", "users")],
                rename_fields=[("people", "fullName", "name")],
                replace_fields=[("people", "age")],
                run=ages,
            )
        ],
    )
```
