---
title: 쿼리
order: 4
---

# 쿼리

쿼리에는 컬렉션의 어떤 객체를 어떤 순서로 몇 개 찾을지 적으며, 코드로 만들 수도 있고 문자열로 쓸 수도 있습니다.

::: tip 연산자를 한곳에서 보기

[쿼리 언어](./query-language.md)에는 문자열 쿼리의 모든 부분을, [쿼리 빌더](./query-builder.md)에는 각 언어 빌더의 모든 부분을 예제와 함께 모았습니다.

:::

## 쿼리 만들기

::: lang rust

`Query`에는 `Filter`와 정렬, 개수 제한, 오프셋이 들어갑니다. 컬렉션의 `query`는 찾은 객체를 돌려주고, `count`는 개수를 셉니다.

```rust
use darudb::{Database, Filter, Query};

fn adults(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);

    for user in users.query(&query)? {
        println!("{:?}", user.get("name"));
    }

    let adults = users.count(&Query::new().filter(Filter::ge("age", 18)))?;
    println!("{adults} adults");
    Ok(())
}
```

조건은 `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `between`, `is_in`, `contains`, `starts_with`, `ends_with`, `is_null`, `is_not_null`이 있고, `and`와 `or`, `!`로 엮습니다. `Filter::eq(field, Value::Null)`은 `is_null`과 같습니다.

:::

::: lang node

`find`, `findOne`, `count`는 받은 `Query`에 조건을 붙이는 함수를 받습니다. `findOne`은 첫 객체에서 멈춥니다.

```ts
const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);

db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null));
  users.find((q) => q.where('tags', 'contains', 'new').where('age', 'between', [18, 30]));
  users.find((q) => q.where('team.city', '==', 'Seoul'));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
  users.count((q) => q.where('name', 'startsWith', 'A'));
});
```

`where`는 필드와 연산자, 값을 받습니다. 연산자는 `==`, `!=`, `<`, `<=`, `>`, `>=`, `between`, `in`, `contains`, `startsWith`, `endsWith`입니다. 다시 부르면 AND로 조건이 붙습니다. 그 밖의 조건은 `where`에 `conditions`를 받는 함수를 넘겨 만듭니다. `conditions`에는 위 연산자마다 메서드가 있고, `isNull`과 `isNotNull`, 그리고 `and`, `or`, `not`이 있습니다.

TypeScript에서 `where`는 컬렉션에 있는 필드와 그 필드 타입의 값만 받습니다. `team.city`처럼 내장 객체나 링크를 지나는 경로는 쿼리를 실행할 때 엔진이 검사합니다.

:::

::: lang dart

`find`, `findOne`, `count`는 생성기가 클래스마다 만든 쿼리 빌더에 조건을 붙이는 함수를 받습니다. 빌더의 필드는 객체이고, 그 메서드가 조건을 만듭니다. 조건은 `&`(둘 다), `|`(둘 중 하나), `~`(부정)로 엮습니다. `findOne`은 첫 객체에서 멈춥니다.

```dart
final adults = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q.where(q.age.atLeast(18)).sortBy(q.age, descending: true).limit(10),
  ),
);

db.read((txn) {
  final users = txn.collection(userSchema);

  users.find((q) => q.where(q.email.isNull()));
  users.find((q) => q.where(q.tags.contains('new') & q.age.between(18, 30)));
  users.find((q) => q.where(q.team.city.equals('Seoul')));
  users.find((q) => q.where(q.name.equals('Alice') | q.email.isNull()));
  users.count((q) => q.where(~q.name.startsWith('A')));
});
```

메서드는 `equals`, `notEquals`, `lessThan`, `atMost`, `greaterThan`, `atLeast`, `between`, `isIn`, `contains`, `startsWith`, `endsWith`, `isNull`, `isNotNull`이 있고, 필드마다 맞는 것만 있습니다. `startsWith`는 문자열에, `contains`는 문자열과 목록에 있습니다. `where`를 다시 부르면 AND로 조건이 붙고, `sortBy`를 다시 부르면 앞의 정렬에서 같은 객체끼리 그 필드로 정렬합니다. 링크 필드 `q.team`은 담긴 키와 비교하고, 대상 컬렉션의 필드도 가집니다. 내장 객체 필드는 그 객체의 필드를 가집니다.

조건은 모두 타입 검사를 거치므로 `q.age.atLeast('18')`은 컴파일되지 않습니다.

:::

::: lang python

`find`, `find_one`, `count`는 조건이나 `Query`를 받습니다. `F`는 `F.age`처럼 필드를 가리키고, 그 필드를 비교하면 조건이 됩니다. 조건은 `&`(둘 다), `|`(둘 중 하나), `~`(부정)로 엮습니다. `where`는 조건을 `Query`로 만들고, `Query`에는 정렬과 오프셋, 개수 제한을 붙입니다. `find_one`은 첫 객체에서 멈춥니다.

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    adults = users.find(where(F.age >= 18).sort_by(F.age, descending=True).limit(10))

    users.find(F.email.is_null())
    users.find(F.tags.contains("new") & F.age.between(18, 30))
    users.find(F.team.city == "Seoul")
    users.find((F.name == "Alice") | F.email.is_null())
    users.count(~F.name.startswith("A"))
```

조건은 `==`, `!=`, `<`, `<=`, `>`, `>=`와 메서드 `between`, `is_in`, `contains`, `startswith`, `endswith`, `is_null`, `is_not_null`로 만듭니다. `== None`과 `!= None`도 null인지 검사합니다. 쿼리에 `where`를 다시 부르면 AND로 조건이 붙고, `sort_by`를 다시 부르면 앞의 정렬에서 같은 객체끼리 그 필드로 정렬합니다. 내장 객체나 링크를 지나는 경로는 `F.address.city`처럼 속성으로 잇고, 이름이 메서드와 같은 필드는 `F["name"]`으로 가리킵니다.

- **괄호.** Python에서 `&`와 `|`는 `==`나 `>=`보다 먼저 묶이므로, 다른 조건과 엮는 비교는 괄호로 감쌉니다. 조건에는 참거짓 값이 없어서 `and`, `or`, `not`을 쓰면 `TypeError`가 납니다.
- **Python 이름.** 경로는 필드를 Python 속성 이름으로 가리키고, 패키지가 그 이름을 파일에 저장된 이름으로 바꿔 엔진에 넘깁니다. `field(name=...)`를 준 필드라면 두 이름이 다릅니다.
- **재사용.** 메서드마다 새 쿼리를 돌려주고, 쿼리는 실행하는 컬렉션마다 한 번만 컴파일됩니다. 그래서 변수에 담아 둔 쿼리는 다음에 실행할 때 다시 컴파일하지 않습니다.
- **검사.** `F`에는 타입이 없으므로, 컬렉션에 없는 필드나 타입이 다른 값을 쓴 쿼리는 실행할 때 `INVALID_QUERY`로 실패합니다.

:::

조건의 뜻은 언어와 관계없이 같습니다.

- **경로**는 필드 이름이고, 내장 객체나 링크를 지날 때는 `.`으로 잇습니다. `address.city`처럼 쓰고, `author.name`처럼 쓰면 링크가 가리키는 객체를 검사합니다. 가리키는 객체가 없으면 null로 읽습니다.
- **목록.** 목록에 건 조건은 원소 하나라도 맞으면 참입니다. 목록에 `contains`를 쓰면 그 원소가 있는지 봅니다. 빈 목록은 null이 아닙니다.
- **null.** null인 필드에 건 조건은 null인지 묻는 조건을 빼고 모두 거짓입니다.
- **타입.** 값은 필드의 타입과 같아야 합니다. 정수 필드는 정수와 비교하고 실수와는 비교하지 않습니다. 실수 필드는 정수와 실수 모두와 비교합니다. 링크는 대상 컬렉션의 키와 비교합니다. 이를 어기거나 없는 필드를 쓴 쿼리는 `INVALID_QUERY`로 실패합니다.
- **순서.** 정렬을 주지 않으면 기본 키 순서로 나오고, 정렬 값이 같은 객체끼리도 기본 키 순서를 따릅니다. null은 오름차순에서 맨 앞, 내림차순에서 맨 뒤에 옵니다. 문자열은 바이트 순서로 비교합니다.

## 문자열로 쿼리 쓰기

같은 쿼리를 쿼리 언어로 쓸 수도 있습니다. 문자열을 해석하는 것은 엔진이므로 어느 언어에서나 똑같이 해석됩니다.

::: lang rust

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let query = Query::parse(
        r#"age >= $0 AND name STARTSWITH "A" SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;
    let _ = query;
    Ok(())
}
```

:::

::: lang node

```ts
users.find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [18, 'A']);
```

패키지는 한 번 해석한 문자열을 256개까지 기억해 두므로, 같은 문자열을 다른 매개변수로 실행할 때는 해석을 건너뜁니다.

:::

::: lang dart

```dart
users.findText(r'age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [18, 'A']);
```

패키지는 한 번 해석한 문자열을 256개까지 기억해 두므로, 같은 문자열을 다른 매개변수로 실행할 때는 해석을 건너뜁니다. `r'...'`처럼 원시 문자열로 써야 Dart가 `$0`을 문자열 보간으로 읽지 않습니다.

:::

::: lang python

```python
users.find("age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10", 18, "A")
```

값은 문자열 뒤에 위치 인자로 넘깁니다. 패키지는 한 번 해석한 문자열을 256개까지 기억해 두므로, 같은 문자열을 다른 매개변수로 실행할 때는 해석을 건너뜁니다. 문자열 쿼리에서는 필드를 파일에 저장된 이름으로 쓰고, `F`로 만든 경로에서는 Python 속성 이름으로 씁니다.

:::

필터를 먼저 쓰고 `SORT BY`, `LIMIT`, `OFFSET`을 차례로 붙이며, 모두 생략할 수 있습니다. 키워드는 대소문자를 가리지 않고, 문자열은 큰따옴표로 감쌉니다. `limit`처럼 키워드와 이름이 같은 필드는 백틱으로 감쌉니다. `$0`, `$1` 같은 매개변수에는 함께 넘긴 값이 순서대로 들어갑니다. 프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요. 문법에 맞지 않는 문자열은 `INVALID_QUERY`로 실패하고, 메시지에 문제가 생긴 글자의 위치가 나옵니다.

## 자주 도는 쿼리 준비하기

값만 바꿔 여러 번 실행할 쿼리는 한 번만 해석해 두고 실행할 때마다 값을 넘길 수 있습니다. 준비한 쿼리를 매개변수 값 없이 실행하면 `INVALID_QUERY`로 실패합니다.

::: lang rust

`Query::prepare`는 `$0`, `$1` 같은 매개변수를 비워 둔 채 쿼리를 만들고, `bind`는 문자열을 다시 해석하지 않고 값을 채웁니다.

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["alice@example.com".into()])?;
    let _ = query;
    Ok(())
}
```

:::

::: lang node

`db.prepare`는 컬렉션과 쿼리를 받습니다. 쿼리는 문자열로 써도 되고, 바뀌는 값 자리에 `param`을 넣어 만들어도 됩니다. 준비한 쿼리와 값을 `find`, `findOne`, `count`에 넘기면 되며, 동기와 비동기 트랜잭션 어디서든 쓸 수 있습니다.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const inAges = db.prepare('users', 'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) => {
  const users = txn.collection('users');

  users.findOne(byEmail, ['alice@example.com']);
  users.find(inAges, [18, 30]);
});
```

준비한 쿼리는 준비할 때 정한 컬렉션에서만 실행됩니다.

:::

::: lang dart

`db.prepare`는 스키마 상수와 문자열을 받습니다. 준비한 쿼리와 값을 `findPrepared`, `findOnePrepared`, `countPrepared`에 넘기면 되며, 동기와 비동기 트랜잭션 어디서든 쓸 수 있습니다.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');
final inAges = db.prepare(userSchema, r'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) {
  final users = txn.collection(userSchema);

  users.findOnePrepared(byEmail, ['alice@example.com']);
  users.findPrepared(inAges, [18, 30]);
});
```

준비한 쿼리는 준비할 때 정한 컬렉션에서만 실행됩니다.

:::

::: lang python

`db.prepare`는 클래스와 쿼리를 받습니다. 쿼리는 문자열로 써도 되고, 바뀌는 값 자리에 `param`을 넣어 만들어도 됩니다. 준비한 쿼리를 값과 함께 `find`, `find_one`, `count`에 넘기면 되며, 동기와 비동기 트랜잭션 어디서든 쓸 수 있습니다.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))
in_ages = db.prepare(User, "age BETWEEN $0 AND $1 SORT BY age")

with db.read() as txn:
    users = txn.collection(User)

    users.find_one(by_email, "alice@example.com")
    users.find(in_ages, 18, 30)
```

준비한 쿼리는 준비할 때 정한 컬렉션에서만 실행되고, 다른 컬렉션에서는 `INVALID_QUERY`로 실패합니다.

:::

준비해 두면 실행할 때마다 쿼리를 해석하거나 인코딩하는 비용이 빠지고, 문자열 쿼리에서 효과가 가장 큽니다. 실행 계획은 값에 맞춰 매번 새로 세웁니다.

## 엔진이 읽는 방법

기본 키나 인덱스가 있는 필드에 건 조건이 나머지 조건과 AND로 이어져 있으면, 엔진은 그 조건에 맞는 객체만 읽습니다. 인덱스가 있는 필드 하나로만 정렬하면 그 순서대로 읽다가 개수 제한에서 멈춥니다. 그렇지 않으면 컬렉션의 객체를 모두 읽습니다. 어느 쪽으로 읽든 결과는 같습니다.

<Diagram name="query" alt="코드로 만든 쿼리와 문자열로 쓴 쿼리는 엔진 안에서 같은 쿼리가 됩니다. 기본 키나 인덱스에 건 조건이 나머지와 AND로 이어져 있으면 인덱스의 그 범위만 읽고, 아니면 인덱스가 있는 필드 하나로만 정렬하는 경우 그 인덱스를 순서대로 훑고, 그것도 아니면 모든 객체를 읽습니다. 그다음 나머지 필터를 적용하고, 필요하면 정렬한 뒤 건너뛸 만큼 건너뛰고 개수 제한에서 멈춥니다." />
