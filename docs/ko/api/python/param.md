---
title: param
order: 12
---

# param

`param`은 빌더로 만든 쿼리에서 값 자리에 매개변수를 넣어, 쿼리를 실행할 때마다 그 값을 받게 합니다.

```python
def param(index: int) -> Param: ...


class Param:
    index: int

    def __init__(self, index: int) -> None: ...
```

`param(0)`은 첫 매개변수로 쿼리 언어의 `$0`과 같고, `param(1)`은 두 번째 매개변수입니다. `index`는 0부터 65535까지의 정수여야 하고, 그 밖의 값은 `INVALID_QUERY`로 실패합니다. 매개변수는 조건이 비교하는 값이면 어디든 들어갈 수 있습니다. 비교의 값, `between`의 양 끝, `is_in`의 원소, `contains`, `startswith`, `endswith`의 값이 그렇습니다. 필드, 정렬, 개수 제한, 오프셋 자리에는 쓸 수 없습니다.

쿼리를 `find`, `find_one`, `count`로 실행할 때마다 쿼리 뒤에 값을 순서대로 넘깁니다. 값을 받지 못한 매개변수가 있거나 값의 타입이 필드와 다르면 `INVALID_QUERY`로 실패합니다. `==`나 `!=`의 매개변수에 `None`을 넘기면, 쿼리에 `None`을 직접 쓴 것처럼 필드가 `None`인지 검사합니다. 다른 자리의 `None`은 `INVALID_QUERY`로 실패합니다.

매개변수가 든 쿼리는 보통 [Database.prepare](./database.md#prepare)로 준비합니다. 그러면 컬렉션에 맞춰 쿼리를 한 번 컴파일하고, 클래스로 타입이 정해진 [Prepared](../../types/python/prepared.md)를 돌려줍니다. 쿼리는 실행한 클래스와 스키마마다 컴파일한 결과를 기억하므로, 빌더로 만든 쿼리는 준비하지 않은 채로 매개변수와 함께 실행해도 됩니다. 준비한 쿼리는 준비할 때 정한 컬렉션에서만 실행되며, 동기든 비동기든, 읽기든 쓰기든 어느 트랜잭션에서나 쓸 수 있습니다. 실행 계획은 엔진이 넘겨받은 값에 맞춰 매번 새로 세웁니다.

```python
from darudb import F, param, where

in_ages = db.prepare(User, where(F.age.between(param(0), param(1))).sort_by(F.age))
by_email = db.prepare(User, F.email == param(0))

with db.read() as txn:
    users = txn.collection(User)

    users.find(in_ages, 18, 30)
    users.find_one(by_email, "alice@example.com")
    users.count(by_email, None)  # 이메일이 없는 사용자 수
```

## Param

```python
class Param:
    index: int

    def __init__(self, index: int) -> None: ...
```

`param`이 돌려주는 값으로, 네이티브 모듈의 클래스입니다. 번호가 같은 두 매개변수는 서로 같고 해시도 같으며, `repr`은 `param(0)` 같은 꼴입니다.

### index

```python
index: int
```

매개변수의 번호이며 0부터 셉니다. 읽기만 할 수 있습니다.
