---
title: Schema
order: 3
---

# Schema

`Schema`는 `Database.open`이 받는 값으로, 데이터베이스의 컬렉션을 그 클래스로 나열하고 버전을 매깁니다.

```python
class Schema:
    version: int
    collections: tuple[type, ...]

    def __init__(self, version: int, collections: Sequence[type]) -> None: ...
```

`version`은 1 이상의 정수이고, 그 밖의 값은 `INVALID_ARGUMENT`로 실패합니다. 스키마를 바꿀 때마다 올리세요. `collections`에는 [`@collection`](./collection.md)을 붙인 클래스를 넣고, 링크가 가리키는 컬렉션도 모두 여기에 있어야 합니다.

스키마를 만들 때 모든 클래스를 읽습니다. 어노테이션과 기본값, 옵션, 클래스에 담긴 내장 클래스까지 읽습니다. 클래스는 한 프로세스에서 한 번만 읽고, 스키마는 엔진이 파일을 열 때 필요한 것을 담아 두므로 여러 번 열 때 스키마 하나를 함께 써도 됩니다. [`@collection`](./collection.md)의 규칙을 어긴 클래스는 여기서 `INVALID_ARGUMENT`로 실패합니다. `@collection`을 붙이지 않은 클래스, 목록에 든 `@embedded` 클래스, 컬렉션 이름이 같은 클래스 둘도 마찬가지입니다. 나머지는 데이터베이스를 열 때 검사합니다. 스키마에 없는 컬렉션을 가리키는 링크나 필드와 타입이 다른 기본값처럼 엔진이 저장할 수 없는 스키마는 그때 `INVALID_ARGUMENT`로 실패합니다.

```python
import darudb
from darudb import field


@darudb.embedded
class Address:
    city: str
    zip: str | None = field(default=None, name="postcode")


@darudb.collection("teams")
class Team:
    name: str = field(primary_key=True)
    city: str | None = None


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    tags: list[str] = field(default_factory=list, index=True)
    team: str | None = field(default=None, link=Team)
    address: Address | None = None


app = darudb.Schema(1, [Team, User])
db = darudb.Database.open("app.darudb", schema=app)
```

이 절의 다른 페이지에 있는 예제는 이 클래스와 이 `db`를 씁니다.

처음 열 때 스키마를 파일에 저장하고, 그 뒤로는 열 때마다 선언한 스키마를 저장된 것과 비교합니다.

- **버전도 내용도 같으면** 할 일이 없습니다. 필드는 이름으로 맞추므로, 클래스를 넣은 순서나 필드를 선언한 순서만 바꾼 것은 변경이 아닙니다.
- **버전은 같은데 내용이 다르면** `SCHEMA_MISMATCH`입니다. 버전을 올리지 않고 스키마를 바꿨기 때문입니다.
- **파일의 버전이 더 높으면** `SCHEMA_TOO_NEW`입니다. 더 새로운 애플리케이션이 쓴 파일이기 때문입니다.
- **파일의 버전이 더 낮으면** 마이그레이션합니다. [Migration](./migration.md)과 가이드의 [마이그레이션](../../guide/migrations.md)에서 설명합니다.

## 속성

### version

```python
version: int
```

스키마 버전입니다.

### collections

```python
collections: tuple[type, ...]
```

받은 순서 그대로의 클래스입니다. 스키마의 `repr`은 `Schema(1, [Team, User])`처럼 클래스를 이름으로 나열합니다.
