---
title: Migration
order: 4
counterpart: /types/node/migration
---

# Migration

`Migration`은 스키마 버전 `version`이 바로 앞 버전에서 무엇이 바뀌는지, 엔진이 알아서 하지 못하는 부분을 적습니다.

```python
@dataclasses.dataclass(frozen=True)
class Migration:
    version: int
    rename_collections: Sequence[tuple[str, str]] = ()
    rename_fields: Sequence[tuple[str, str, str]] = ()
    delete_collections: Sequence[str] = ()
    replace_fields: Sequence[tuple[str, str]] = ()
    run: Callable[[Migrating], None] | Callable[[AsyncMigrating], Awaitable[None]] | None = None
```

[`Database.open`](./database.md#open)의 `migrations` 옵션이 이것의 목록을 받습니다. 더 낮은 스키마 버전을 담은 파일을 열면 선언한 버전까지의 모든 단계를 버전 순서대로 쓰기 트랜잭션 하나 안에서 실행합니다. 어디서든 실패하면 파일은 예전 스키마와 데이터를 그대로 유지합니다.

새 컬렉션 만들기, 선택 필드나 기본값이 있는 새 필드 추가, 새 인덱스 만들기와 없어진 인덱스 지우기, 없어진 필드 정리는 엔진이 알아서 합니다. 컬렉션이나 필드의 이름 바꾸기, 컬렉션 삭제, 타입이 바뀐 필드는 마이그레이션에 적습니다. 없어진 컬렉션이나 타입이 바뀐 필드를 적지 않으면 `INVALID_ARGUMENT`로 실패하고, 앞 버전의 스키마에 없는 이름을 적어도 마찬가지입니다. `rename_fields` 없이 필드 이름만 바꾸면 필드를 지우고 새로 만든 것으로 보므로, 값은 새 이름을 따라가지 않습니다. 단계 안의 이름은 모두 그 단계 전에 파일에 있던 이름입니다. 그래서 단계에서 이름을 바꾸는 컬렉션의 필드는 컬렉션의 예전 이름 아래에 적고, `field(name=...)`로 선언한 필드는 파일 안의 이름으로 적습니다.

아래 마이그레이션은 `people`에 `fullName`과 문자열 `age`가 있던 버전 1의 파일을 버전 2로 옮깁니다.

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

## 필드

### version

```python
version: int
```

이 단계가 이르는 스키마 버전입니다. 2부터 선언한 스키마의 버전까지의 정수여야 하고, 다른 숫자를 주거나 같은 버전의 마이그레이션이 둘이면 `INVALID_ARGUMENT`로 실패합니다. 스키마 없이 마이그레이션을 주어도 마찬가지입니다.

### rename_collections

```python
rename_collections: Sequence[tuple[str, str]] = ()
```

컬렉션의 예전 이름과 새 이름의 쌍입니다. 컬렉션의 객체는 제자리에 있으므로, 객체가 아무리 많아도 이름 바꾸기는 아무것도 복사하지 않습니다. 다른 컬렉션이 이미 쓰는 이름으로 바꾸면 `INVALID_ARGUMENT`로 실패합니다.

### rename_fields

```python
rename_fields: Sequence[tuple[str, str, str]] = ()
```

단계 전의 이름으로 적은 컬렉션, 필드의 예전 이름, 새 이름입니다. 레코드에는 필드 이름이 아니라 필드 id가 들어 있으므로 다시 쓰는 객체는 없습니다.

### delete_collections

```python
delete_collections: Sequence[str] = ()
```

이 단계에서 객체와 함께 없앨 컬렉션입니다. 새 스키마에서 뺀 컬렉션은 여기에 적어야 합니다. 객체는 `run`이 끝난 뒤 맨 마지막에 지우므로, 그 전까지 `run`은 `previous`로 객체를 읽을 수 있습니다.

### replace_fields

```python
replace_fields: Sequence[tuple[str, str]] = ()
```

단계 전의 이름으로 적은 컬렉션과, 같은 이름의 새 필드로 교체할 필드의 쌍입니다. 필드의 타입이 바뀔 때 씁니다. 교체한 필드는 필드를 지우고 새로 만든 것과 같아서, `run`이 값을 주기 전까지는 기본값이나 `None`이 들어 있습니다. 새 필드가 필수라면 기본값이 있어야 합니다. 예전 값은 `run`에서 `previous`로 읽을 수 있습니다. 기본 키는 교체할 수 없습니다.

### run

```python
run: Callable[[Migrating], None] | Callable[[AsyncMigrating], Awaitable[None]] | None = None
```

마이그레이션의 쓰기 트랜잭션 안에서 실행되는 함수입니다. 단계의 이름 바꾸기와 엔진이 알아서 하는 변경이 끝난 뒤에 실행됩니다. [Migrating](./migrating.md)으로 새 스키마의 컬렉션을 쓰고, `previous`와 `previous_keys`로 예전 스키마대로 읽은 객체를 봅니다. 객체를 쓰면 새 스키마의 필드만 남으므로, 쓰기 전에 이렇게 읽어 두세요. 함수가 있는 버전 단계마다 버전 순서대로 함수 하나가 실행됩니다.

- `Database.open`에서는 평범한 함수여야 합니다. 코루틴 함수는 `INVALID_ARGUMENT`로 실패하고, 오류 메시지가 `open_async`를 쓰라고 알려 줍니다.
- `Database.open_async`에서는 코루틴 함수여도 되고, 이 함수는 [AsyncMigrating](./migrating.md#asyncmigrating)을 받습니다. 평범한 함수도 됩니다.
- 함수가 예외를 일으키면 마이그레이션을 버리고, 파일은 예전 스키마와 데이터를 그대로 유지하며, `open`도 같은 예외를 일으킵니다.
