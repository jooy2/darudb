---
title: 쿼리 빌더
order: 6
pageClass: reference-page
---

# 쿼리 빌더

쿼리 빌더는 각 언어의 코드로 조건을 하나씩 붙여 쿼리를 만듭니다. 이 페이지는 사이드바에서 고른 언어의 빌더를 빠짐없이 모으고, 같은 쿼리를 [쿼리 언어](./query-language.md)로 쓴 형태를 옆에 붙였습니다.

빌더로 만든 쿼리와 문자열로 쓴 쿼리는 엔진 안에서 같은 쿼리가 됩니다. 조건이 null과 목록, 타입을 어떻게 다루는지, 엔진이 인덱스를 어떻게 쓰는지처럼 두 형태에 공통인 내용은 [쿼리](./queries.md)에 있습니다. 빌더를 이루는 타입은 API 섹션에 하나씩 페이지가 있고, 아래 각 부분에서 그 페이지로 이어집니다.

## 쿼리 실행하기

::: lang rust

[`Query`](../api/rust/query.md)는 [`Filter`](../api/rust/filter.md)와 정렬, 개수 제한, 건너뛰기를 담고, 컬렉션이 이를 실행합니다. 만드는 단계에서는 실패하지 않습니다. 실행할 때 컬렉션의 스키마와 맞춰 보고, 없는 필드나 다른 타입의 값이 있으면 그때 `INVALID_QUERY`로 실패합니다.

```rust
use darudb::{Database, Filter, Query};

fn find(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let adults = Query::new().filter(Filter::ge("age", 18));

    let found = users.query(&adults)?; // Vec<Object>
    let first = users.query(&adults.clone().first())?; // at most one
    let how_many = users.count(&adults)?; // u64

    println!("{found:?} {first:?} {how_many}");
    Ok(())
}
```

`collection_of::<T>()`도 같은 쿼리를 실행하며, `#[derive(Object)]`를 붙인 구조체 `T`로 객체를 돌려줍니다.

:::

::: lang node

`find`와 `findOne`, `count`는 함수를 받고, 그 함수가 넘겨받은 [`Query`](../api/node/query.md)에 쿼리를 만듭니다. TypeScript에서는 빌더가 컬렉션의 필드와 그 타입의 값만 받습니다.

```ts
db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('age', '>=', 18)); // every adult
  users.findOne((q) => q.where('email', '==', 'ada@example.com')); // the first, or null
  users.count((q) => q.where('age', '>=', 18)); // a number
});
```

`new Query()`로 호출 밖에서 쿼리를 만들어 함수 대신 넘길 수도 있습니다.

:::

::: lang dart

`find`와 `findOne`, `count`는 함수를 받고, 그 함수가 생성기가 클래스에 맞춰 쓴 [쿼리 빌더](../api/dart/query-builder.md)에 쿼리를 만듭니다. 클래스의 필드마다 [필드 객체](../api/dart/fields.md)가 있고, 그 메서드가 조건을 만듭니다. 타입이 모든 조건을 검사하므로 `q.age.atLeast('18')`은 컴파일되지 않습니다.

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  users.find((q) => q.where(q.age.atLeast(18))); // every adult
  users.findOne((q) => q.where(q.email.equals('ada@example.com'))); // the first, or null
  users.count((q) => q.where(q.age.atLeast(18))); // an int
});
```

:::

::: lang python

`find`와 `find_one`, `count`는 조건을 받거나, 조건에 정렬과 개수 제한, 건너뛰기를 더한 [`Query`](../api/python/query.md)를 받습니다. [`F`](../api/python/conditions.md)로 필드를 가리키고, 값과 비교하면 조건이 됩니다.

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    users.find(F.age >= 18)  # every adult
    users.find_one(F.email == "ada@example.com")  # the first, or None
    users.count(F.age >= 18)  # an int
    users.find(where(F.age >= 18).sort_by(F.age).limit(10))
```

`F`에는 타입이 없으므로, 클래스에 없는 필드나 다른 타입의 값은 쿼리를 실행할 때 `INVALID_QUERY`로 실패합니다.

:::

## 조건

::: lang rust

`Filter`의 연관 함수는 저마다 경로로 가리킨 필드 하나를 검사합니다. 값은 `Value`로 바뀌는 타입이면 됩니다. 정수와 `f64`, `bool`, `&str`과 `String`, 그리고 바이트에는 `&[u8]`이나 `Vec<u8>`을 씁니다.

| 빌더 | 문자열 | 찾는 객체 |
| --- | --- | --- |
| `Filter::eq("city", "Seoul")` | `city == "Seoul"` | 필드가 값과 같은 객체 |
| `Filter::ne("city", "Seoul")` | `city != "Seoul"` | 필드가 null이 아니고 값과 다른 객체 |
| `Filter::lt("age", 18)` | `age < 18` | 필드가 값보다 작은 객체 |
| `Filter::le("age", 18)` | `age <= 18` | 필드가 값보다 작거나 같은 객체 |
| `Filter::gt("age", 18)` | `age > 18` | 필드가 값보다 큰 객체 |
| `Filter::ge("age", 18)` | `age >= 18` | 필드가 값보다 크거나 같은 객체 |
| `Filter::between("age", 20, 29)` | `age BETWEEN 20 AND 29` | 필드가 두 값 사이에 있는 객체(두 값 포함) |
| `Filter::is_in("city", ["Busan", "Lisbon"])` | `city IN ["Busan", "Lisbon"]` | 필드가 값 가운데 하나와 같은 객체 |
| `Filter::contains("tags", "admin")` | `tags CONTAINS "admin"` | 필드가 그 글자나 그 원소를 담은 객체 |
| `Filter::starts_with("name", "J")` | `name STARTSWITH "J"` | 필드가 그 글자로 시작하는 객체 |
| `Filter::ends_with("email", "@example.com")` | `email ENDSWITH "@example.com"` | 필드가 그 글자로 끝나는 객체 |
| `Filter::is_null("email")` | `email IS NULL` | 필드가 null인 객체 |
| `Filter::is_not_null("email")` | `email IS NOT NULL` | 필드에 값이 있는 객체 |

`Filter::eq(field, Value::Null)`은 `is_null`이고, `Filter::ne(field, Value::Null)`은 `is_not_null`입니다.

:::

::: lang node

`where`는 필드와 연산자, 값을 받습니다. `where`로 쓸 수 없는 조건은 [`conditions`](../api/node/conditions.md)를 받는 함수로 넘기며, 거기에 검사마다 메서드가 있습니다.

| `where` | `conditions` | 문자열 | 찾는 객체 |
| --- | --- | --- | --- |
| `where('city', '==', 'Seoul')` | `c.eq('city', 'Seoul')` | `city == "Seoul"` | 필드가 값과 같은 객체 |
| `where('city', '!=', 'Seoul')` | `c.ne('city', 'Seoul')` | `city != "Seoul"` | 필드가 null이 아니고 값과 다른 객체 |
| `where('age', '<', 18)` | `c.lt('age', 18)` | `age < 18` | 필드가 값보다 작은 객체 |
| `where('age', '<=', 18)` | `c.le('age', 18)` | `age <= 18` | 필드가 값보다 작거나 같은 객체 |
| `where('age', '>', 18)` | `c.gt('age', 18)` | `age > 18` | 필드가 값보다 큰 객체 |
| `where('age', '>=', 18)` | `c.ge('age', 18)` | `age >= 18` | 필드가 값보다 크거나 같은 객체 |
| `where('age', 'between', [20, 29])` | `c.between('age', 20, 29)` | `age BETWEEN 20 AND 29` | 필드가 두 값 사이에 있는 객체(두 값 포함) |
| `where('city', 'in', ['Busan', 'Lisbon'])` | `c.in('city', ['Busan', 'Lisbon'])` | `city IN ["Busan", "Lisbon"]` | 필드가 값 가운데 하나와 같은 객체 |
| `where('tags', 'contains', 'admin')` | `c.contains('tags', 'admin')` | `tags CONTAINS "admin"` | 필드가 그 글자나 그 원소를 담은 객체 |
| `where('name', 'startsWith', 'J')` | `c.startsWith('name', 'J')` | `name STARTSWITH "J"` | 필드가 그 글자로 시작하는 객체 |
| `where('email', 'endsWith', '@example.com')` | `c.endsWith('email', '@example.com')` | `email ENDSWITH "@example.com"` | 필드가 그 글자로 끝나는 객체 |
| `where('email', '==', null)` | `c.isNull('email')` | `email IS NULL` | 필드가 null인 객체 |
| `where('email', '!=', null)` | `c.isNotNull('email')` | `email IS NOT NULL` | 필드에 값이 있는 객체 |

`bytes` 필드는 `Uint8Array`와 비교하고, 링크는 링크한 컬렉션의 키와 비교합니다.

:::

::: lang dart

필드 객체는 필드 타입에 맞는 메서드를 가집니다. 모든 필드는 null을 검사하고, 값 필드는 같은지를, 순서가 있는 필드는 크기를, 문자열은 글자를, 목록은 원소를 검사합니다.

| 빌더 | 문자열 | 찾는 객체 |
| --- | --- | --- |
| `q.city.equals('Seoul')` | `city == "Seoul"` | 필드가 값과 같은 객체 |
| `q.city.notEquals('Seoul')` | `city != "Seoul"` | 필드가 null이 아니고 값과 다른 객체 |
| `q.age.lessThan(18)` | `age < 18` | 필드가 값보다 작은 객체 |
| `q.age.atMost(18)` | `age <= 18` | 필드가 값보다 작거나 같은 객체 |
| `q.age.greaterThan(18)` | `age > 18` | 필드가 값보다 큰 객체 |
| `q.age.atLeast(18)` | `age >= 18` | 필드가 값보다 크거나 같은 객체 |
| `q.age.between(20, 29)` | `age BETWEEN 20 AND 29` | 필드가 두 값 사이에 있는 객체(두 값 포함) |
| `q.city.isIn(['Busan', 'Lisbon'])` | `city IN ["Busan", "Lisbon"]` | 필드가 값 가운데 하나와 같은 객체 |
| `q.name.contains('in')` | `name CONTAINS "in"` | 필드가 그 글자를 담은 객체 |
| `q.name.startsWith('J')` | `name STARTSWITH "J"` | 필드가 그 글자로 시작하는 객체 |
| `q.email.endsWith('@example.com')` | `email ENDSWITH "@example.com"` | 필드가 그 글자로 끝나는 객체 |
| `q.email.isNull()` | `email IS NULL` | 필드가 null인 객체 |
| `q.email.isNotNull()` | `email IS NOT NULL` | 필드에 값이 있는 객체 |

목록 필드는 원소를 검사하며, 원소 하나라도 성립하면 성립합니다.

| 빌더 | 문자열 | 찾는 객체 |
| --- | --- | --- |
| `q.tags.contains('admin')` | `tags CONTAINS "admin"` | 목록에 그 원소가 있는 객체 |
| `q.tags.containsAny(['admin', 'editor'])` | `tags IN ["admin", "editor"]` | 목록에 값 가운데 하나와 같은 원소가 있는 객체 |
| `q.tags.anyStartsWith('team-')` | `tags STARTSWITH "team-"` | 목록에 그 글자로 시작하는 원소가 있는 객체 |
| `q.tags.anyEndsWith('-lead')` | `tags ENDSWITH "-lead"` | 목록에 그 글자로 끝나는 원소가 있는 객체 |

:::

::: lang python

`F.field`를 값과 비교하면 조건이 되고, `F.field`의 메서드도 조건을 만듭니다.

| 빌더 | 문자열 | 찾는 객체 |
| --- | --- | --- |
| `F.city == "Seoul"` | `city == "Seoul"` | 필드가 값과 같은 객체 |
| `F.city != "Seoul"` | `city != "Seoul"` | 필드가 null이 아니고 값과 다른 객체 |
| `F.age < 18` | `age < 18` | 필드가 값보다 작은 객체 |
| `F.age <= 18` | `age <= 18` | 필드가 값보다 작거나 같은 객체 |
| `F.age > 18` | `age > 18` | 필드가 값보다 큰 객체 |
| `F.age >= 18` | `age >= 18` | 필드가 값보다 크거나 같은 객체 |
| `F.age.between(20, 29)` | `age BETWEEN 20 AND 29` | 필드가 두 값 사이에 있는 객체(두 값 포함) |
| `F.city.is_in(["Busan", "Lisbon"])` | `city IN ["Busan", "Lisbon"]` | 필드가 값 가운데 하나와 같은 객체 |
| `F.tags.contains("admin")` | `tags CONTAINS "admin"` | 필드가 그 글자나 그 원소를 담은 객체 |
| `F.name.startswith("J")` | `name STARTSWITH "J"` | 필드가 그 글자로 시작하는 객체 |
| `F.email.endswith("@example.com")` | `email ENDSWITH "@example.com"` | 필드가 그 글자로 끝나는 객체 |
| `F.email.is_null()`, `F.email == None` | `email IS NULL` | 필드가 null인 객체 |
| `F.email.is_not_null()`, `F.email != None` | `email IS NOT NULL` | 필드에 값이 있는 객체 |

`F["between"]`은 메서드와 이름이 같은 필드 `between`을 가리킵니다.

:::

## 조건 묶기

::: lang rust

| 빌더       | 문자열    | 성립하는 경우           |
| ---------- | --------- | ----------------------- |
| `a.and(b)` | `a AND b` | 둘 다 성립할 때         |
| `a.or(b)`  | `a OR b`  | 하나라도 성립할 때      |
| `!a`       | `NOT a`   | 조건이 성립하지 않을 때 |

```rust
use darudb::Filter;

fn main() {
    let in_either_city = Filter::eq("city", "Seoul").or(Filter::eq("city", "Busan"));
    let adults_there = in_either_city.and(Filter::ge("age", 18));
    let elsewhere = !Filter::eq("city", "Seoul");
    let _ = (adults_there, elsewhere);
}
```

호출은 쓴 순서대로 묶이므로 우선순위 규칙이 따로 없습니다. `a.or(b).and(c)`는 `(a OR b) AND c`입니다.

:::

::: lang node

| 빌더                                 | 문자열    | 성립하는 경우           |
| ------------------------------------ | --------- | ----------------------- |
| `q.where(a).where(b)`, `c.and(a, b)` | `a AND b` | 둘 다 성립할 때         |
| `c.or(a, b)`                         | `a OR b`  | 하나라도 성립할 때      |
| `c.not(a)`                           | `NOT a`   | 조건이 성립하지 않을 때 |

```ts
users.find((q) =>
  q.where((c) => c.or(c.eq('city', 'Seoul'), c.eq('city', 'Busan'))).where('age', '>=', 18)
);
users.find((q) => q.where((c) => c.not(c.eq('city', 'Seoul'))));
```

`where`를 부를 때마다 조건이 AND로 붙습니다. `and`와 `or`는 조건을 몇 개든 받습니다.

:::

::: lang dart

| 빌더                             | 문자열    | 성립하는 경우           |
| -------------------------------- | --------- | ----------------------- |
| `a & b`, 또는 `where`를 한 번 더 | `a AND b` | 둘 다 성립할 때         |
| `a \| b`                         | `a OR b`  | 하나라도 성립할 때      |
| `~a`                             | `NOT a`   | 조건이 성립하지 않을 때 |

```dart
users.find(
  (q) => q.where(
    (q.city.equals('Seoul') | q.city.equals('Busan')) & q.age.atLeast(18),
  ),
);
users.find((q) => q.where(~q.city.equals('Seoul')));
```

우선순위는 Dart 연산자를 따릅니다. `~`가 가장 먼저 묶이고, 그다음 `&`, 마지막이 `|`입니다.

:::

::: lang python

| 빌더                             | 문자열    | 성립하는 경우           |
| -------------------------------- | --------- | ----------------------- |
| `a & b`, 또는 `where`를 한 번 더 | `a AND b` | 둘 다 성립할 때         |
| `a \| b`                         | `a OR b`  | 하나라도 성립할 때      |
| `~a`                             | `NOT a`   | 조건이 성립하지 않을 때 |

```python
users.find(((F.city == "Seoul") | (F.city == "Busan")) & (F.age >= 18))
users.find(~(F.city == "Seoul"))
```

Python에서는 `&`와 `|`가 `==`나 `>=`보다 먼저 묶이므로, 다른 조건과 묶는 비교는 괄호로 감쌉니다. 조건에는 참거짓 값이 없어서 `and`와 `or`, `not`을 쓰면 `TypeError`가 납니다.

:::

문자열과 마찬가지로 조건을 뒤집으면 필드가 null인 객체도 찾습니다. `NOT city == "Seoul"`은 그런 객체를 찾고, `city != "Seoul"`은 찾지 않습니다.

## 정렬, 개수 제한, 건너뛰기

::: lang rust

| 빌더                   | 문자열             | 하는 일                        |
| ---------------------- | ------------------ | ------------------------------ |
| `.sort_by("name")`     | `SORT BY name`     | 필드로 오름차순 정렬           |
| `.sort_by_desc("age")` | `SORT BY age DESC` | 필드로 내림차순 정렬           |
| `.limit(10)`           | `LIMIT 10`         | 객체를 최대 그 개수까지 돌려줌 |
| `.offset(20)`          | `OFFSET 20`        | 그 개수만큼 먼저 건너뜀        |
| `.first()`             | `LIMIT 1`          | 첫 객체 하나만 돌려줌          |

```rust
use darudb::{Filter, Query};

fn main() {
    let page = Query::new()
        .filter(Filter::eq("city", "Seoul"))
        .sort_by("name")
        .sort_by_desc("age")
        .limit(20)
        .offset(40);
    let _ = page;
}
```

정렬은 앞서 준 정렬 다음에 적용됩니다. `limit`와 `offset`은 나중에 부른 것이 앞의 것을 대신합니다.

:::

::: lang node

| 빌더                     | 문자열             | 하는 일                        |
| ------------------------ | ------------------ | ------------------------------ |
| `.sortBy('name')`        | `SORT BY name`     | 필드로 오름차순 정렬           |
| `.sortBy('age', 'desc')` | `SORT BY age DESC` | 필드로 내림차순 정렬           |
| `.limit(10)`             | `LIMIT 10`         | 객체를 최대 그 개수까지 돌려줌 |
| `.offset(20)`            | `OFFSET 20`        | 그 개수만큼 먼저 건너뜀        |

```ts
users.find((q) =>
  q.where('city', '==', 'Seoul').sortBy('name').sortBy('age', 'desc').limit(20).offset(40)
);
```

정렬은 앞서 준 정렬 다음에 적용됩니다. `findOne`은 쿼리가 찾은 첫 객체를 돌려줍니다.

:::

::: lang dart

| 빌더                               | 문자열             | 하는 일                        |
| ---------------------------------- | ------------------ | ------------------------------ |
| `.sortBy(q.name)`                  | `SORT BY name`     | 필드로 오름차순 정렬           |
| `.sortBy(q.age, descending: true)` | `SORT BY age DESC` | 필드로 내림차순 정렬           |
| `.limit(10)`                       | `LIMIT 10`         | 객체를 최대 그 개수까지 돌려줌 |
| `.offset(20)`                      | `OFFSET 20`        | 그 개수만큼 먼저 건너뜀        |

```dart
users.find(
  (q) => q
      .where(q.city.equals('Seoul'))
      .sortBy(q.name)
      .sortBy(q.age, descending: true)
      .limit(20)
      .offset(40),
);
```

정렬은 앞서 준 정렬 다음에 적용됩니다. `findOne`은 쿼리가 찾은 첫 객체를 돌려줍니다.

:::

::: lang python

| 빌더                               | 문자열             | 하는 일                        |
| ---------------------------------- | ------------------ | ------------------------------ |
| `.sort_by(F.name)`                 | `SORT BY name`     | 필드로 오름차순 정렬           |
| `.sort_by(F.age, descending=True)` | `SORT BY age DESC` | 필드로 내림차순 정렬           |
| `.limit(10)`                       | `LIMIT 10`         | 객체를 최대 그 개수까지 돌려줌 |
| `.offset(20)`                      | `OFFSET 20`        | 그 개수만큼 먼저 건너뜀        |

```python
from darudb import F, Query, where

users.find(
    where(F.city == "Seoul").sort_by(F.name).sort_by(F.age, descending=True).limit(20).offset(40)
)
users.find(Query().sort_by(F.age, descending=True).limit(10))  # no filter
```

`where`는 조건에서 쿼리를 시작하고, `Query()`는 조건 없이 시작합니다. 메서드마다 새 쿼리를 돌려주므로, 변수에 둔 쿼리 하나에서 여러 쿼리를 이어 만들 수 있습니다.

:::

## 경로

::: lang rust

| 빌더                                   | 문자열                     | 검사하는 것        |
| -------------------------------------- | -------------------------- | ------------------ |
| `Filter::eq("address.city", "Lisbon")` | `address.city == "Lisbon"` | 내장 객체의 필드   |
| `Filter::eq("team.name", "Core")`      | `team.name == "Core"`      | 링크한 객체의 필드 |
| `Filter::eq("team", 3)`                | `team == 3`                | 링크가 가진 키     |

경로는 문자열에서처럼 필드 이름을 `.`으로 이은 것이며, 이름은 최대 32개입니다.

:::

::: lang node

| 빌더                                    | 문자열                     | 검사하는 것        |
| --------------------------------------- | -------------------------- | ------------------ |
| `where('address.city', '==', 'Lisbon')` | `address.city == "Lisbon"` | 내장 객체의 필드   |
| `where('team.name', '==', 'Core')`      | `team.name == "Core"`      | 링크한 객체의 필드 |
| `where('team', '==', 3)`                | `team == 3`                | 링크가 가진 키     |

TypeScript는 컬렉션 자신의 필드를 검사하고, 점으로 이은 경로는 엔진이 쿼리를 실행할 때 검사합니다.

:::

::: lang dart

| 빌더                              | 문자열                     | 검사하는 것        |
| --------------------------------- | -------------------------- | ------------------ |
| `q.address.city.equals('Lisbon')` | `address.city == "Lisbon"` | 내장 객체의 필드   |
| `q.team.name.equals('Core')`      | `team.name == "Core"`      | 링크한 객체의 필드 |
| `q.team.equals(3)`                | `team == 3`                | 링크가 가진 키     |

링크의 필드 객체는 링크한 클래스의 필드를, 내장 객체의 필드 객체는 그 클래스의 필드를 가지므로 경로도 타입이 검사합니다.

:::

::: lang python

| 빌더                         | 문자열                     | 검사하는 것        |
| ---------------------------- | -------------------------- | ------------------ |
| `F.address.city == "Lisbon"` | `address.city == "Lisbon"` | 내장 객체의 필드   |
| `F.team.name == "Core"`      | `team.name == "Core"`      | 링크한 객체의 필드 |
| `F.team == 3`                | `team == 3`                | 링크가 가진 키     |

경로는 필드를 Python 속성 이름으로 가리키고, 패키지가 이를 파일에 저장된 이름으로 바꿔 엔진에 넘깁니다. `field(name=...)`로 저장 이름을 바꿨다면 그 이름이 쓰입니다.

:::

## 자주 도는 쿼리 준비하기

::: lang rust

빌더는 만들 때 값을 받으므로, 값이 바뀌는 쿼리는 문자열로 쓰고 준비합니다. `Query::prepare`가 한 번만 해석하고, 실행할 때마다 `bind`로 값을 줍니다.

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["ada@example.com".into()])?;
    let _ = query;
    Ok(())
}
```

:::

::: lang node

[`param`](../api/node/param.md)은 바뀌는 값의 자리를 맡고, `db.prepare`가 쿼리를 한 번만 인코딩합니다. 준비한 쿼리는 실행할 때마다 값을 받습니다.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const inAges = db.prepare('users', (q) => q.where('age', 'between', [param(0), param(1)]));

db.read((txn) => {
  const users = txn.collection('users');

  users.findOne(byEmail, ['ada@example.com']);
  users.find(inAges, [18, 30]);
});
```

:::

::: lang dart

빌더는 만들 때 값을 받으므로, 값이 바뀌는 쿼리는 문자열로 쓰고 준비합니다. `db.prepare`가 한 번만 해석하고, 실행할 때마다 `findPrepared`와 `findOnePrepared`, `countPrepared`로 값을 줍니다.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');

db.read((txn) {
  txn.collection(userSchema).findOnePrepared(byEmail, ['ada@example.com']);
});
```

:::

::: lang python

[`param`](../api/python/param.md)은 바뀌는 값의 자리를 맡고, `db.prepare`가 쿼리를 한 번만 컴파일합니다. 준비한 쿼리는 실행할 때마다 그 뒤에 값을 받습니다.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))
in_ages = db.prepare(User, F.age.between(param(0), param(1)))

with db.read() as txn:
    users = txn.collection(User)

    users.find_one(by_email, "ada@example.com")
    users.find(in_ages, 18, 30)
```

:::

## 예제

[쿼리 언어의 예제](./query-language.md#예제)에 나온 쿼리를 같은 사용자 컬렉션에 대해 코드로 만든 모습입니다. `name`과 `city`는 문자열, `email`은 고유 인덱스가 있는 선택 문자열, `age`는 인덱스가 있는 정수, `tags`는 문자열 목록, `address`는 내장 객체, `team`은 팀으로 가는 링크입니다.

::: lang rust

| 문자열 | 빌더 |
| --- | --- |
| `email == "ada@example.com"` | `Query::new().filter(Filter::eq("email", "ada@example.com"))` |
| `age >= 18 AND age < 30` | `Query::new().filter(Filter::ge("age", 18).and(Filter::lt("age", 30)))` |
| `city IN ["Seoul", "Busan"]` | `Query::new().filter(Filter::is_in("city", ["Seoul", "Busan"]))` |
| `name STARTSWITH "Ma" SORT BY name` | `Query::new().filter(Filter::starts_with("name", "Ma")).sort_by("name")` |
| `tags CONTAINS "admin"` | `Query::new().filter(Filter::contains("tags", "admin"))` |
| `email IS NULL` | `Query::new().filter(Filter::is_null("email"))` |
| `NOT city == "Seoul"` | `Query::new().filter(!Filter::eq("city", "Seoul"))` |
| `address.city == "Lisbon"` | `Query::new().filter(Filter::eq("address.city", "Lisbon"))` |
| `team.name == "Core"` | `Query::new().filter(Filter::eq("team.name", "Core"))` |
| `SORT BY age DESC, name LIMIT 10` | `Query::new().sort_by_desc("age").sort_by("name").limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `Query::new().filter(Filter::eq("city", "Seoul")).sort_by("name").limit(20).offset(40)` |

:::

::: lang node

| 문자열 | 빌더 |
| --- | --- |
| `email == "ada@example.com"` | `(q) => q.where('email', '==', 'ada@example.com')` |
| `age >= 18 AND age < 30` | `(q) => q.where('age', '>=', 18).where('age', '<', 30)` |
| `city IN ["Seoul", "Busan"]` | `(q) => q.where('city', 'in', ['Seoul', 'Busan'])` |
| `name STARTSWITH "Ma" SORT BY name` | `(q) => q.where('name', 'startsWith', 'Ma').sortBy('name')` |
| `tags CONTAINS "admin"` | `(q) => q.where('tags', 'contains', 'admin')` |
| `email IS NULL` | `(q) => q.where('email', '==', null)` |
| `NOT city == "Seoul"` | `(q) => q.where((c) => c.not(c.eq('city', 'Seoul')))` |
| `address.city == "Lisbon"` | `(q) => q.where('address.city', '==', 'Lisbon')` |
| `team.name == "Core"` | `(q) => q.where('team.name', '==', 'Core')` |
| `SORT BY age DESC, name LIMIT 10` | `(q) => q.sortBy('age', 'desc').sortBy('name').limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `(q) => q.where('city', '==', 'Seoul').sortBy('name').limit(20).offset(40)` |

:::

::: lang dart

| 문자열 | 빌더 |
| --- | --- |
| `email == "ada@example.com"` | `(q) => q.where(q.email.equals('ada@example.com'))` |
| `age >= 18 AND age < 30` | `(q) => q.where(q.age.atLeast(18) & q.age.lessThan(30))` |
| `city IN ["Seoul", "Busan"]` | `(q) => q.where(q.city.isIn(['Seoul', 'Busan']))` |
| `name STARTSWITH "Ma" SORT BY name` | `(q) => q.where(q.name.startsWith('Ma')).sortBy(q.name)` |
| `tags CONTAINS "admin"` | `(q) => q.where(q.tags.contains('admin'))` |
| `email IS NULL` | `(q) => q.where(q.email.isNull())` |
| `NOT city == "Seoul"` | `(q) => q.where(~q.city.equals('Seoul'))` |
| `address.city == "Lisbon"` | `(q) => q.where(q.address.city.equals('Lisbon'))` |
| `team.name == "Core"` | `(q) => q.where(q.team.name.equals('Core'))` |
| `SORT BY age DESC, name LIMIT 10` | `(q) => q.sortBy(q.age, descending: true).sortBy(q.name).limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `(q) => q.where(q.city.equals('Seoul')).sortBy(q.name).limit(20).offset(40)` |

:::

::: lang python

| 문자열 | 빌더 |
| --- | --- |
| `email == "ada@example.com"` | `F.email == "ada@example.com"` |
| `age >= 18 AND age < 30` | `(F.age >= 18) & (F.age < 30)` |
| `city IN ["Seoul", "Busan"]` | `F.city.is_in(["Seoul", "Busan"])` |
| `name STARTSWITH "Ma" SORT BY name` | `where(F.name.startswith("Ma")).sort_by(F.name)` |
| `tags CONTAINS "admin"` | `F.tags.contains("admin")` |
| `email IS NULL` | `F.email.is_null()` |
| `NOT city == "Seoul"` | `~(F.city == "Seoul")` |
| `address.city == "Lisbon"` | `F.address.city == "Lisbon"` |
| `team.name == "Core"` | `F.team.name == "Core"` |
| `SORT BY age DESC, name LIMIT 10` | `Query().sort_by(F.age, descending=True).sort_by(F.name).limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `where(F.city == "Seoul").sort_by(F.name).limit(20).offset(40)` |

:::
