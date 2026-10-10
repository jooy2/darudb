---
title: QueryInput
order: 7
group: queries
pageClass: reference-page
---

# QueryInput

`QueryInput`은 `find`, `find_one`, `count`가 쿼리로 받는 값으로, 빌더로 만든 쿼리, 조건, 문자열, 준비한 쿼리 가운데 하나이거나 아무것도 아닙니다.

```python
QueryInput: TypeAlias = "Query | Condition | str | Prepared[Any] | None"
```

[ReadCollection](../../api/python/read-collection.md)은 이 값을 위치 인자로만 받는 첫 인자로 받고, 그 뒤에 쿼리 매개변수의 값을 받습니다.

- **`None`이나 인자 없음**: `find()`는 모든 객체를 기본 키 순서로 돌려주고, `count()`는 모든 객체를 셉니다.
- **[Query](../../api/python/query.md)**: `where(F.age >= 18).sort_by(F.age)`처럼 만들고, 한 번 만들어 여러 번 넘길 수 있습니다.
- **[Condition](../../api/python/conditions.md)**: `F.age >= 18`처럼 만들며, 그 조건 하나만 있는 쿼리로 칩니다.
- **`str`**: [쿼리 언어](../../api/python/query.md#쿼리-언어)로 쓴 쿼리이고, `$0`, `$1` 같은 매개변수를 씁니다.
- **[Prepared](./prepared.md) 쿼리**: `Database.prepare`로 같은 컬렉션에 준비한 쿼리입니다.

이 밖의 값은 `INVALID_QUERY`로 실패합니다. 쿼리로 무엇을 할 수 있는지는 [쿼리](../../guide/queries.md)에 있습니다.

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    users.find(where(F.age >= 18).sort_by(F.age, descending=True).limit(10))
    users.count(F.email.is_null())
    users.find("age >= $0 AND name STARTSWITH $1", 18, "A")
```

## 매개변수

```python
*parameters: object
```

쿼리 매개변수의 값으로, 쿼리 뒤에 위치 인자로 차례로 넘기며 `$0`이나 `param(0)`이 첫 번째입니다. 각각은 조건이 비교하는 값 하나이고, 엔진에서의 타입은 Python 타입이 정합니다.

- `bool`은 불리언이고, `int`는 정수이며 64비트 안에 들어가야 합니다.
- `float`는 부동소수점 수입니다. `int` 필드는 정수와만 비교하고, `float` 필드는 어떤 수와도 비교합니다.
- `str`은 문자열이고, `bytes`, `bytearray`, `memoryview`는 바이트입니다.
- `None`은 `==`나 `!=`로 비교하는 매개변수에만 줄 수 있고, 그러면 필드가 `None`인지를 봅니다. 다른 곳에서는 `INVALID_QUERY`로 실패합니다.

값을 받지 못한 매개변수가 있으면 `INVALID_QUERY`로 실패합니다. 필드와 타입이 다른 값이나 목록처럼 값 하나가 아닌 것을 넘겨도 마찬가지입니다. 매개변수가 없는 쿼리에 넘긴 값은 무시합니다.

```python
users.find_one("email == $0", None)  # email이 없는 사용자
```

프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요.
