---
title: TinyDB
order: 9
languages: [python]
---

# TinyDB에서 옮기기

TinyDB 파일의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 테이블과 문서가 컬렉션과 객체에 어떻게 대응하는지, 모든 문서를 복사하는 스크립트, 그리고 애플리케이션의 쿼리가 어떻게 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| TinyDB | DaruDB |
| --- | --- |
| 데이터베이스. JSON 파일 하나 | 데이터베이스. 파일 하나 |
| 테이블 | 컬렉션 |
| 문서 | 객체 |
| 문서의 `doc_id` | 정수인 자동 `id` |
| 다른 테이블의 `doc_id`를 담는 필드 | 링크. 그 객체의 `id`를 담습니다 |
| 문자열, 숫자, `true`나 `false`, `null` | 문자열, 정수나 실수, 불리언 필드. `null`이 있으면 선택 필드 |
| 리스트 | 값이나 링크의 목록 |
| 중첩된 딕셔너리 | 키가 정해져 있으면 내장 객체 |
| 어떤 문서에는 없는 필드 | 선택 필드, 또는 기본값이 있는 필드 |
| `Query()` | 쿼리. `F`로 만들거나 쿼리 언어로 씁니다 |
| `CachingMiddleware` | 정할 것이 없습니다. 자주 읽는 것은 페이지 캐시가 둡니다 |

- **필드에 타입이 생깁니다.** TinyDB 문서는 어떤 JSON이든 담을 수 있고 한 테이블의 문서끼리 모양이 달라도 되지만, DaruDB 필드는 자기 타입의 값만 담습니다. 복사하기 전에 테이블마다 타입이 다른 값을 찾아 스크립트에서 바꾸세요. 그러지 않으면 첫 값에서 `INVALID_ARGUMENT`로 멈춥니다.
- **필드는 모두 선언합니다.** 어떤 문서에는 없는 필드는 선택 필드로 두거나 기본값을 주고, 몇몇 문서에만 있는 필드도 똑같이 선언합니다.
- **키를 그대로 옮깁니다.** `doc_id`를 객체의 `id`로 옮기면 문서 사이의 참조가 모두 링크로 살아남습니다. 컬렉션은 옮겨 넣은 가장 큰 `id` 다음부터 번호를 이어서 매깁니다.
- **인덱스가 생깁니다.** TinyDB 쿼리는 테이블의 문서를 모두 읽습니다. 애플리케이션이 쿼리에 쓰는 필드마다 인덱스를 선언하면, 엔진은 맞는 객체만 읽습니다.

## 스키마 선언하기

예제로 옮길 파일에는 애플리케이션이 쓴 테이블이 둘 있습니다. `users`에는 `name`, `email`, `age`가 있고, `posts`에는 `slug`, 사용자의 `doc_id`인 `author`, `body`, `published_at`, `tags` 목록이 있습니다.

`users`는 `doc_id`를 자동 `id`로 그대로 쓰고, `author`는 링크가 됩니다.

```python
import darudb
from darudb import field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


@darudb.collection("posts")
class Post:
    slug: str = field(primary_key=True)
    author: int | None = field(default=None, link=User, index=True)
    body: str
    published_at: str | None = None
    tags: list[str] = field(index=True)


app = darudb.Schema(1, [User, Post])
```

글은 `doc_id`로 가리키는 곳이 없으므로 `slug`를 기본 키로 씁니다. 다른 문서가 가리키는 테이블은 `users`처럼 `doc_id`를 `id`로 남깁니다.

## 문서 복사하기

스크립트는 TinyDB로 파일을 읽습니다.

```python
import darudb
from tinydb import TinyDB

from schema import Post, User, app

old = TinyDB("app.json", access_mode="r")
db = darudb.Database.open("app.darudb", schema=app)


def copy(documents, cls, convert):
    """``documents``를 트랜잭션마다 천 개씩 ``cls``의 컬렉션에 복사합니다."""
    batch = []

    def flush():
        with db.write(durability="deferred") as txn:
            txn.collection(cls).insert_many(batch)

        batch.clear()

    for document in documents:
        batch.append(convert(document))

        if len(batch) == 1000:
            flush()

    if batch:
        flush()


copy(
    old.table("users"),
    User,
    lambda doc: User(id=doc.doc_id, name=doc["name"], email=doc.get("email"), age=doc.get("age", 0)),
)

copy(
    old.table("posts"),
    Post,
    lambda doc: Post(
        slug=doc["slug"],
        author=doc.get("author"),
        body=doc["body"],
        published_at=doc.get("published_at"),
        tags=doc.get("tags", []),
    ),
)

# 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close()
old.close()
```

`access_mode="r"`로 열면 파일은 읽기 전용이 되어 그대로 남습니다. TinyDB는 테이블을 열 때 파일 전체를 메모리에 읽으므로, 복사에도 애플리케이션이 쓰던 만큼의 메모리가 듭니다. 필드가 없는 문서는 `doc.get`에 준 기본값을 받습니다.

## 복사본 확인하기

테이블마다 `len(old.table("users"))`를 컬렉션의 `count`와 견주고, 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

TinyDB 쿼리(여기서는 `User = Query()`, `Post = Query()`)는 쓰던 모양 그대로 `F`로 만든 쿼리가 되고, 쿼리 언어로도 같은 것을 쓸 수 있습니다.

| TinyDB                                | DaruDB 빌더               | DaruDB 쿼리 언어        |
| ------------------------------------- | ------------------------- | ----------------------- |
| `User.age >= 18`                      | `F.age >= 18`             | `age >= 18`             |
| `(User.age >= 18) & (User.age <= 30)` | `F.age.between(18, 30)`   | `age BETWEEN 18 AND 30` |
| `User.email == None`                  | `F.email.is_null()`       | `email IS NULL`         |
| `User.name.matches("A.*")`            | `F.name.startswith("A")`  | `name STARTSWITH "A"`   |
| `User.name.search("li")`              | `F.name.contains("li")`   | `name CONTAINS "li"`    |
| `Post.tags.any(["news"])`             | `F.tags.contains("news")` | `tags CONTAINS "news"`  |
| `table.get(doc_id=7)`                 | `get(7)`                  |                         |
| `table.count(q)`                      | `count(q)`                | 같은 쿼리로 `count`     |

- `matches`와 `search`는 정규식을 받는데, 정규식에는 대응하는 것이 없습니다. 흔한 경우는 `STARTSWITH`, `ENDSWITH`, `CONTAINS`로 됩니다. 대소문자를 가리지 않고 찾으려면 필드를 소문자로 바꾼 사본을 두고 그것으로 쿼리하세요.
- TinyDB에서는 `search`가 돌려준 목록을 애플리케이션이 정렬했지만, DaruDB에서는 `where(F.age >= 18).sort_by(F.age, descending=True).limit(10)`처럼 엔진이 정렬하고 개수를 자릅니다.
- 링크를 거치는 조건은 링크된 객체를 읽습니다. `posts`에서 `F.author.name == "Alice"`처럼 씁니다.

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **쓰기는 트랜잭션입니다.** `with db.write() as txn:`은 블록이 끝나면 커밋하고 예외가 나면 중단하므로, 변경 여럿이 함께 들어가거나 하나도 들어가지 않습니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **객체는 컬렉션 클래스의 인스턴스**이고, 얼린 데이터클래스입니다. 바꿀 때는 `update(key, age=31)`로 필드를 정하거나, `dataclasses.replace`로 만든 객체를 `put`합니다.
- **쓸 때마다 파일 전체를 다시 쓰지 않습니다.** 커밋은 바뀐 페이지만 쓰므로, 데이터가 커져도 쓰기는 느려지지 않습니다.
- **여러 프로세스**가 한 파일을 함께 쓸 수 있습니다. 쓰는 쪽은 한 번에 하나, 읽는 쪽은 몇이든 됩니다. [여러 프로세스](../guide/processes.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
