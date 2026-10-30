---
title: linked
order: 13
---

# linked

`linked`는 다른 컬렉션, 또는 같은 컬렉션에서 어떤 객체를 가리키는 객체에 대한 조건을 만듭니다.

```python
def linked(
    linking: type | str, link: str | FieldRef, condition: Condition | None = None
) -> Condition: ...
```

이 조건은 컬렉션의 클래스나 이름인 `linking`의 어떤 객체가 링크 필드 `link`로 가리키고, 주어졌다면 `linking`의 속성으로 쓴 `condition`도 만족하는 객체에서 참이 됩니다. 가리키는 객체 하나가 조건 전체를 만족해야 하며, `~`를 씌우면 아무것도 가리키지 않는 객체를 찾습니다. 링크는 속성 이름, `"meta.author"` 같은 경로, 또는 `F` 경로이며, 쿼리를 실행하는 컬렉션을 가리키는 단일 링크나 링크 목록이어야 합니다. 패키지는 `linking`의 클래스를 거쳐 파일에 저장된 이름으로 바꿔 엔진에 넘기고, 이름으로 준 컬렉션이면 이름을 그대로 넘깁니다.

스키마에 없는 컬렉션이나 그런 링크가 아닌 필드는 쿼리를 실행할 때 `INVALID_QUERY`로 실패하고, 클래스도 이름도 아닌 `linking`이나 빈 링크는 곧바로 실패합니다. [쿼리 언어](../../guide/query-language.md#이-객체를-가리키는-객체)의 `ANY posts.author (year >= 2020)`과 같습니다.

```python
from darudb import F, linked

with db.read() as txn:
    users = txn.collection(User)

    users.find(linked(Post, "author", F.year >= 2020))
    users.find(~linked(Post, "author"))
    users.find(linked(Post, "readers") & (F.age > 40))
```
