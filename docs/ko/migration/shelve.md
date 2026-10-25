---
title: shelve
order: 10
languages: [python]
---

# shelve에서 옮기기

`shelve` 파일의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 셸프의 키와 pickle한 값이 컬렉션과 객체에 어떻게 대응하는지, 모든 항목을 복사하는 스크립트, 그리고 애플리케이션에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| `shelve` | DaruDB |
| --- | --- |
| 셸프. 뒤에 있는 `dbm` 모듈에 따라 파일 하나나 몇 개 | 데이터베이스. 파일 하나 |
| `user:`로 시작하는 키처럼 한 종류의 키 | 컬렉션 |
| 키 | 기본 키. 타입은 정수, 문자열, 바이트 중 하나이고, `user:` 같은 접두사는 뗍니다 |
| pickle한 값. 딕셔너리나 애플리케이션 클래스의 인스턴스 | 객체. 딕셔너리의 키나 속성마다 필드를 둡니다 |
| 다른 항목의 키를 담는 값 | 링크. 그 객체의 키를 담습니다 |
| `shelf[key] = value` | 쓰기 트랜잭션 안의 `put` |
| `sync`, `close` | 커밋. 바뀐 것만 씁니다 |
| `writeback=True` | 대응하는 것 없음. 객체는 `update`나 `put`으로 바꿉니다 |

- **필드에 타입이 생깁니다.** pickle은 무엇이든 담을 수 있고 같은 종류의 키 아래 값끼리 모양이 달라도 되지만, DaruDB 필드는 자기 타입의 값만 담습니다. 복사하기 전에 셸프에서 타입이 다른 값을 찾아 스크립트에서 바꾸세요. 그러지 않으면 첫 값에서 `INVALID_ARGUMENT`로 멈춥니다.
- **스크립트에는 pickle이 요구하는 것이 있어야 합니다.** 값을 unpickle하면 그 값을 만든 클래스를 가져오므로, 애플리케이션의 객체를 담은 셸프는 애플리케이션 모듈을 가져올 수 있는 곳에서, 값을 쓸 때의 클래스 그대로 읽어야 합니다.
- **같은 `dbm` 모듈이 필요합니다.** 셸프는 `dbm` 파일이고, 어느 모듈이 썼는지는 플랫폼과 Python 버전에 따라 다릅니다. Python 3.13부터는 `dbm.sqlite3`, 빌드에 들어 있다면 `dbm.gnu`나 `dbm.ndbm`, 아니면 `dbm.dumb`입니다. `dbm.whichdb("app")`이 어느 것인지 알려 주며, 스크립트는 그 모듈이 있는 Python에서 돌립니다.
- **인덱스가 생깁니다.** 셸프는 키로만 읽으므로, 필드로 값을 찾던 애플리케이션은 키를 모두 훑거나 딕셔너리를 따로 두었습니다. 애플리케이션이 찾을 때 쓰는 필드마다 인덱스를 선언하면, 엔진은 맞는 객체만 읽습니다.

## 스키마 선언하기

예제로 옮길 셸프에는 애플리케이션이 쓴 키가 두 종류 있습니다. `user:<id>`의 값은 `name`, `email`, `age`를 담은 딕셔너리이고, `post:<slug>`의 값은 사용자의 id인 `author_id`, `body`, `published_at`, `tags` 목록을 담은 딕셔너리입니다.

`users`는 id를 자동 `id`로 그대로 쓰고, `author_id`는 링크가 됩니다.

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

## 항목 복사하기

스크립트는 `shelve`로 셸프를 읽습니다.

```python
import shelve

import darudb

from schema import Post, User, app

old = shelve.open("app", flag="r")
db = darudb.Database.open("app.darudb", schema=app)


def copy(objects, cls):
    """``objects``를 트랜잭션마다 천 개씩 ``cls``의 컬렉션에 복사합니다."""
    batch = []

    def flush():
        with db.write(durability="deferred") as txn:
            txn.collection(cls).insert_many(batch)

        batch.clear()

    for obj in objects:
        batch.append(obj)

        if len(batch) == 1000:
            flush()

    if batch:
        flush()


def users():
    for key in old:
        if key.startswith("user:"):
            value = old[key]
            yield User(id=int(key[5:]), name=value["name"], email=value["email"], age=value["age"])


def posts():
    for key in old:
        if key.startswith("post:"):
            value = old[key]
            yield Post(
                slug=key[5:],
                author=value["author_id"],
                body=value["body"],
                published_at=value["published_at"],
                tags=value["tags"],
            )


copy(users(), User)
copy(posts(), Post)

# 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close()
old.close()
```

`flag="r"`로 열면 셸프는 읽기 전용이 되어 그대로 남습니다. 값이 애플리케이션 클래스의 인스턴스라면 `value["name"]` 대신 `value.name`처럼 속성으로 읽으면 됩니다.

## 복사본 확인하기

종류마다 키를 세어(`sum(1 for key in old if key.startswith("user:"))`) 컬렉션의 `count`와 견줍니다. 그다음 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

셸프는 키로 읽습니다. 애플리케이션이 그 밖에 하던 일은 쿼리가 됩니다.

| 애플리케이션이 하던 일 | DaruDB |
| --- | --- |
| `old[f"user:{id}"]` | `users`에서 `get(id)` |
| 어떤 이메일을 쓰는 사용자를 찾으려고 `user:` 키를 모두 훑음 | 고유 인덱스를 거치는 `find_one(F.email == email)` |
| `user:` 키를 모두 훑으며 나이를 검사함 | `find(F.age.between(18, 30))` |
| 어떤 태그가 있는 글을 찾으려고 `post:` 키를 모두 훑음 | `posts`에서 `find(F.tags.contains("news"))` |
| 사용자마다 글을 모은 딕셔너리를 따로 둠 | `posts`에서 인덱스를 거치는 `find(F.author == id)` |

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **쓰기는 트랜잭션입니다.** `with db.write() as txn:`은 블록이 끝나면 커밋하고 예외가 나면 중단하므로, 변경 여럿이 함께 들어가거나 하나도 들어가지 않습니다. 비정상 종료가 나도 파일은 마지막 커밋이 남긴 그대로입니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **객체는 컬렉션 클래스의 인스턴스**이고, 얼린 데이터클래스입니다. 바꿀 때는 `writeback` 대신 `update(key, age=31)`로 필드를 정하거나, `dataclasses.replace`로 만든 객체를 `put`합니다.
- **pickle을 쓰지 않습니다.** 파일에는 엔진이 애플리케이션 코드를 하나도 가져오지 않고 읽는 레코드가 들어 있습니다. 그래서 클래스 이름을 바꾸거나 옮겨도 옛 데이터를 읽을 수 있고, 속성 이름을 바꿀 때는 `field(name="...")`로 파일에 저장된 이름을 그대로 둡니다.
- **여러 프로세스와 스레드**가 한 파일을 함께 쓸 수 있습니다. 쓰는 쪽은 한 번에 하나, 읽는 쪽은 몇이든 됩니다. [여러 프로세스](../guide/processes.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
