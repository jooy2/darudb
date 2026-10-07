---
title: Query
order: 10
counterpart: /api/dart/query-builder
---

# Query

`Query`는 컬렉션의 `find`, `find_one`, `count`가 어떤 객체를 어떤 순서로 몇 개 찾을지 적습니다.

```python
class Query:
    def __init__(self) -> None: ...
    def where(self, condition: Condition) -> Query: ...
    def sort_by(self, field: str | FieldRef, *, descending: bool = False) -> Query: ...
    def offset(self, count: int) -> Query: ...
    def limit(self, count: int) -> Query: ...


def where(condition: Condition) -> Query: ...
```

쿼리에는 필터, 정렬, 오프셋, 개수 제한이 있고 모두 생략할 수 있습니다. `Query()`에는 아무것도 없으므로 모든 객체를 찾습니다. 함수 `where(condition)`은 `Query().where(condition)`과 같고, 쿼리는 보통 이 함수로 시작합니다. 메서드는 모두 새 쿼리를 돌려주고 부른 쿼리는 그대로 두므로, 쿼리를 보관하거나 함께 쓰거나 다른 쿼리로 넓혀도 됩니다. 정렬을 주지 않으면 기본 키 순서로 나오고, 정렬 값이 같은 객체끼리도 기본 키 순서를 따릅니다.

쿼리는 [F](./conditions.md)로 Python 속성 이름을 써서 필드를 가리키고, 패키지가 엔진에는 파일에 저장된 이름을 넘깁니다. 쿼리는 컬렉션의 클래스로 처음 실행할 때 컴파일되고, 클래스와 스키마마다 컴파일한 결과를 기억해 둡니다. 그래서 보관해 둔 쿼리를 다시 실행하면 다시 컴파일하지 않고, 예전 스키마의 클래스와 새 스키마의 클래스로 실행하면 각각 컴파일합니다. 컬렉션에 없는 필드를 쓴 쿼리처럼 스키마에 맞지 않는 쿼리는 실행할 때 `INVALID_QUERY`로 실패합니다.

```python
from darudb import F, where

adults = where(F.age >= 18)
page = adults.sort_by(F.name).offset(20).limit(10)

with db.read() as txn:
    users = txn.collection(User)

    users.count(adults)
    users.find(page)
```

기본 키나 인덱스가 있는 필드에 건 조건이 나머지 필터와 `&`로 이어져 있으면, 엔진은 그 조건에 맞는 객체만 읽습니다. 인덱스가 있는 필드 하나로만 정렬하면 그 순서대로 읽다가 개수 제한에서 멈춥니다. 어느 쪽이든 결과는 같습니다. 자세한 설명은 [쿼리](../../guide/queries.md)에 있습니다.

## 메서드

### where

```python
def where(self, condition: Condition) -> Query: ...
```

`F`로 만든 [Condition](./conditions.md)인 `condition`에 맞는 객체만 남깁니다. 앞서 `where`에 준 조건도 모두 지켜야 하며, 부를 때마다 앞의 조건과 AND로 이어집니다. 조건이 아닌 값을 주면 `INVALID_QUERY`로 실패합니다.

```python
from darudb import F, where

query = where(F.age.between(18, 30)).where(F.tags.contains("new") | F.email.is_null())
```

### sort_by

```python
def sort_by(self, field: str | FieldRef, *, descending: bool = False) -> Query: ...
```

`field`로 정렬합니다. `descending`이 아니면 오름차순이고, 앞서 준 정렬 다음 순위로 적용됩니다. 필드는 `F.address.city` 같은 `F` 경로이거나, 같은 경로를 `"address.city"`처럼 속성 이름을 점으로 이은 문자열로 씁니다. `None`은 오름차순에서 맨 앞, 내림차순에서 맨 뒤에 오고, 문자열은 UTF-8 바이트 순서로 정렬됩니다. 목록이나 내장 객체로 정렬하거나, 여러 객체를 가리키는 링크를 지나 정렬하면 쿼리를 실행할 때 `INVALID_QUERY`로 실패합니다.

### offset

```python
def offset(self, count: int) -> Query: ...
```

정렬한 결과에서 처음 `count`개를 건너뜁니다. 다시 부르면 앞의 값을 대신합니다. `count`가 음수이면 그 자리에서 `INVALID_QUERY`로 실패합니다.

### limit

```python
def limit(self, count: int) -> Query: ...
```

객체를 최대 `count`개까지 돌려줍니다. 다시 부르면 앞의 값을 대신하고, `count`의 규칙은 `offset`과 같습니다.

## 쿼리 언어

`find`, `find_one`, `count`, `Database.prepare`는 쿼리를 문자열로도 받습니다. 엔진은 이 문자열을 빌더가 만드는 것과 같은 쿼리로 해석합니다. 컬렉션은 문자열에 들어가지 않고, 쿼리를 실행하는 호출이 정합니다. 문자열은 파일에 저장된 이름으로 필드를 가리키므로, `field(name=...)`로 선언한 필드는 저장된 이름으로 씁니다.

```python
with db.read() as txn:
    txn.collection(User).find(
        'age >= $0 AND (name STARTSWITH "A" OR email IS NULL) SORT BY age DESC LIMIT 10', 18
    )
```

- **순서.** 필터를 먼저 쓰고, 그다음 `SORT BY`와 쉼표로 구분한 필드, `LIMIT`, `OFFSET`을 차례로 씁니다. 필드 뒤에 `DESC`를 붙이지 않으면 오름차순입니다. 각 부분은 생략할 수 있습니다.
- **조건.** `field == value`와 `!=`, `<`, `<=`, `>`, `>=`, 그리고 `field BETWEEN a AND b`, `field IN [a, b]`, `field CONTAINS value`, `STARTSWITH`, `ENDSWITH`, `field IS NULL`, `field IS NOT NULL`이 있습니다. 조건은 `AND`, `OR`, `NOT`과 괄호로 엮고, `AND`가 `OR`보다 먼저 묶입니다. 필드 자리에는 점으로 이은 경로도 쓸 수 있습니다.
- **값.** 정수에는 소수점이 없고, 실수에는 소수점이나 지수가 있으며, 둘 다 `-`로 시작할 수 있습니다. 문자열은 큰따옴표로 감싸고, `\"`, `\\`, `\n`, `\t`, `\u{...}` 이스케이프를 씁니다. 그 밖의 값은 `true`, `false`, `null`입니다.
- **매개변수.** `$0`, `$1` 같은 매개변수에는 문자열 뒤에 넘긴 값이 순서대로 들어갑니다. 프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요.
- **이름.** 키워드는 대소문자를 가리지 않습니다. `limit`처럼 키워드와 이름이 같은 필드는 경로의 첫머리에서 백틱으로 감쌉니다.
- **오류.** 해석할 수 없는 문자열은 `INVALID_QUERY`로 실패하고, 메시지에 문제가 생긴 글자의 위치가 1부터 센 번호로 나옵니다. 괄호와 `NOT`은 48단계까지만 중첩할 수 있습니다.

패키지는 마지막으로 해석한 문자열 256개를 기억하므로, 같은 문자열을 다시 실행하면 해석을 건너뜁니다.
