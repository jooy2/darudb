---
title: Prepared
order: 6
group: queries
pageClass: reference-page
---

# Prepared

`Prepared`는 컬렉션 하나에 대해 한 번 컴파일해 두고, 실행할 때마다 매개변수의 값을 주는 쿼리입니다.

```python
class Prepared(Generic[T]):
    collection: str
```

[`Database.prepare`](../../api/python/database.md#prepare)가 만듭니다. `$0`, `$1` 같은 매개변수가 든 쿼리 언어 문자열로 만들 수도 있고, 바뀌는 값 자리에 [`param`](../../api/python/param.md)을 넣어 만든 [Query](../../api/python/query.md)나 [조건](../../api/python/conditions.md)으로 만들 수도 있습니다. `find`, `find_one`, `count`가 이 쿼리를 받고, 그 뒤에 매개변수의 값을 받습니다. `T`는 컬렉션의 클래스이므로, 타입 검사기는 준비한 쿼리가 찾는 객체의 타입을 압니다.

준비한 쿼리는 데이터베이스도 트랜잭션도 붙잡고 있지 않습니다. 그래서 동기든 비동기든, 읽기든 쓰기든 어느 트랜잭션에서나 실행할 수 있습니다. 문자열을 준비해 두면 실행할 때마다 해석하는 비용이 빠집니다. 패키지는 `find`, `find_one`, `count`에 바로 넘긴 문자열도 마지막 256개까지 해석해 두므로, 문자열을 준비하는 효과는 그보다 많은 문자열을 실행하는 프로그램에서 가장 큽니다. 빌더로 만든 쿼리는 준비하든 안 하든 클래스와 스키마마다 컴파일한 결과를 기억합니다. 그래서 빌더로 만든 쿼리를 준비하면 컬렉션을 미리 한 번 확인하고, 클래스로 타입이 정해진 쿼리를 얻습니다. 실행 계획은 엔진이 받은 값에 맞춰 매번 새로 세웁니다.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))
in_ages = db.prepare(User, "age BETWEEN $0 AND $1 SORT BY age")

with db.read() as txn:
    users = txn.collection(User)

    users.find_one(by_email, "alice@example.com")
    users.find(in_ages, 18, 30)
```

- `prepare`는 스키마에 없는 컬렉션이면 `INVALID_ARGUMENT`, 해석할 수 없는 문자열이면 `INVALID_QUERY`, 데이터베이스를 닫은 뒤라면 `CLOSED`로 실패합니다.
- 다른 컬렉션에서 실행하거나, 값을 받지 못한 매개변수가 있거나, 쿼리의 필드나 값이 스키마와 맞지 않으면 실행할 때 `INVALID_QUERY`로 실패합니다.

## 속성

### collection

```python
collection: str
```

쿼리를 준비한 컬렉션의 이름이자, 쿼리를 실행할 수 있는 유일한 컬렉션입니다. 준비한 쿼리의 `repr`은 `Prepared('users')`처럼 이 이름을 보여 줍니다.
