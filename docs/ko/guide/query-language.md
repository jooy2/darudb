---
title: 쿼리 언어
order: 5
pageClass: reference-page
---

# 쿼리 언어

쿼리 언어는 쿼리 전체를 문자열 하나로 씁니다. 문자열은 엔진이 해석하므로 어느 언어에서나 뜻이 같습니다. 이 페이지는 쿼리 언어의 모든 부분을 예제와 함께 모았습니다.

문자열로 쓴 쿼리와 [쿼리 빌더](./query-builder.md)로 만든 쿼리는 엔진 안에서 같은 쿼리가 됩니다. 그래서 쿼리를 받는 곳이면 어디에나 둘 중 무엇이든 넘길 수 있고, 한 프로그램 안에서 섞어 써도 됩니다. 조건이 null과 목록, 타입을 어떻게 다루는지, 엔진이 인덱스를 어떻게 쓰는지처럼 두 형태에 공통인 내용은 [쿼리](./queries.md)에 있습니다.

## 한눈에 보기

```text
age >= 18 AND city == "Seoul" SORT BY age DESC, name LIMIT 20 OFFSET 40
```

| 부분      | 예                              | 없으면                |
| --------- | ------------------------------- | --------------------- |
| 필터      | `age >= 18 AND city == "Seoul"` | 컬렉션의 모든 객체    |
| `SORT BY` | `SORT BY age DESC, name`        | 기본 키 순서          |
| `LIMIT`   | `LIMIT 20`                      | 필터가 찾은 모든 객체 |
| `OFFSET`  | `OFFSET 40`                     | 첫 객체부터           |

부분마다 빼도 되지만, 쓰는 부분은 이 순서를 지켜야 합니다. 빈 문자열도 쿼리이며 모든 객체를 찾습니다. 컬렉션은 문자열에 들어가지 않고, 쿼리를 실행하는 호출이 정합니다.

## 문자열 쿼리 실행하기

::: lang rust

`Query::parse`는 문자열과 매개변수 값을 받습니다. 돌려받은 쿼리는 다른 쿼리처럼 `query`나 `count`에 넘깁니다.

```rust
use darudb::{Database, Query};

fn find(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::parse("age >= $0 SORT BY age DESC LIMIT 10", &[18.into()])?;

    println!("{:?}", users.query(&query)?);
    Ok(())
}
```

:::

::: lang node

`find`와 `findOne`, `count`는 문자열과 매개변수 값의 배열을 받습니다.

```ts
db.read((txn) => {
  const users = txn.collection('users');

  users.find('age >= $0 SORT BY age DESC LIMIT 10', [18]);
  users.findOne('email == $0', ['ada@example.com']);
  users.count('city == "Seoul"');
});
```

:::

::: lang dart

`findText`와 `findOneText`, `countText`는 문자열과 매개변수 값의 목록을 받습니다. 원시 문자열 `r'...'`로 쓰면 Dart가 `$0`을 문자열 보간으로 읽지 않습니다.

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  users.findText(r'age >= $0 SORT BY age DESC LIMIT 10', [18]);
  users.findOneText(r'email == $0', ['ada@example.com']);
  users.countText('city == "Seoul"');
});
```

:::

::: lang python

`find`와 `find_one`, `count`는 문자열을 받고, 그 뒤에 매개변수 값을 차례로 받습니다.

```python
with db.read() as txn:
    users = txn.collection(User)

    users.find("age >= $0 SORT BY age DESC LIMIT 10", 18)
    users.find_one("email == $0", "ada@example.com")
    users.count('city == "Seoul"')
```

문자열은 필드를 파일에 저장된 이름으로 부릅니다. `field(name=...)`로 다른 이름을 주지 않았다면 Python 이름과 같습니다.

:::

## 조건

조건은 이름이나 [경로](#필드와-경로)로 가리킨 필드 하나를 검사하며, 성립하거나 성립하지 않습니다. null인 필드에 대한 조건은 null 검사를 빼면 모두 성립하지 않습니다.

| 연산자 | 예 | 찾는 객체 |
| --- | --- | --- |
| `==` | `city == "Seoul"` | 필드가 값과 같은 객체 |
| `!=` | `city != "Seoul"` | 필드가 null이 아니고 값과 다른 객체 |
| `<`, `<=`, `>`, `>=` | `age >= 18` | 필드가 값보다 작거나, 작거나 같거나, 크거나, 크거나 같은 객체 |
| `BETWEEN` ... `AND` ... | `age BETWEEN 20 AND 29` | 필드가 두 값 사이에 있는 객체. 두 값도 포함합니다 |
| `IN [` ... `]` | `city IN ["Busan", "Lisbon"]` | 필드가 값 가운데 하나와 같은 객체. `IN []`은 아무것도 찾지 않습니다 |
| `CONTAINS` | `name CONTAINS "in"` | 필드가 그 글자를 담은 문자열이거나 그 값을 담은 목록인 객체 |
| `STARTSWITH` | `name STARTSWITH "J"` | 필드가 그 글자로 시작하는 문자열인 객체 |
| `ENDSWITH` | `email ENDSWITH "@example.com"` | 필드가 그 글자로 끝나는 문자열인 객체 |
| `IS NULL` | `email IS NULL` | 필드가 null인 객체. 객체에서 빠진 필드도 null입니다 |
| `IS NOT NULL` | `email IS NOT NULL` | 필드에 값이 있는 객체 |
| `== null`, `!= null` | `email == null` | `IS NULL`, `IS NOT NULL`과 같습니다 |

- **문자열**은 바이트로 비교하므로 `"Z"`가 `"a"`보다 앞에 옵니다. `STARTSWITH`와 `ENDSWITH`, `CONTAINS`도 대소문자까지 정확히 맞아야 합니다. `STARTSWITH`와 `ENDSWITH`를 문자열도 문자열 목록도 아닌 필드에 쓰면 `INVALID_QUERY`로 실패하고, `CONTAINS`를 문자열도 목록도 아닌 필드에 써도 마찬가지입니다.
- **목록**에 대한 조건은 원소 하나라도 성립하면 성립합니다. `tags == "admin"`과 `tags CONTAINS "admin"`은 모두 그 원소를 가진 객체를 찾고, `tags STARTSWITH "ad"`는 그렇게 시작하는 원소를 가진 객체를 찾습니다. 빈 목록은 null이 아니며 원소가 없습니다.
- **값은 필드의 타입을 따릅니다.** 정수 필드는 정수하고만 비교하고 실수와는 비교하지 않습니다. 실수 필드는 실수나 -2^53부터 2^53까지의 정수와, 링크는 링크한 컬렉션의 키와 비교합니다. 이와 다른 값을 쓰거나 `null`에 `==`과 `!=` 말고 다른 연산자를 쓰면 `INVALID_QUERY`로 실패합니다.

## 조건 묶기

| 키워드      | 예                                   | 성립하는 경우                |
| ----------- | ------------------------------------ | ---------------------------- |
| `AND`       | `age >= 18 AND city == "Seoul"`      | 둘 다 성립할 때              |
| `OR`        | `city == "Seoul" OR city == "Busan"` | 하나라도 성립할 때           |
| `NOT`       | `NOT city == "Seoul"`                | 뒤의 조건이 성립하지 않을 때 |
| `(` ... `)` | `(a == 1 OR b == 2) AND c == 3`      | 괄호대로 묶어서              |

- **우선순위.** `NOT`이 가장 먼저 묶이고, 그다음이 `AND`, 마지막이 `OR`입니다. 그래서 `a == 1 OR b == 2 AND c == 3`은 `a == 1 OR (b == 2 AND c == 3)`입니다.
- **`NOT`과 null.** `NOT city == "Seoul"`은 city가 null인 객체도 찾습니다. 뒤집은 조건이 그 객체에서 성립하지 않기 때문입니다. `city != "Seoul"`은 그런 객체를 찾지 않습니다.
- **깊이.** 문자열 안에서 괄호와 `NOT`은 48단계까지 겹칠 수 있고, 그렇게 만든 필터는 24단계까지 겹칠 수 있습니다. `AND` 안의 `AND`, `OR` 안의 `OR`은 한 단계로 셉니다.

## 정렬, 개수 제한, 건너뛰기

| 절 | 예 | 하는 일 |
| --- | --- | --- |
| `SORT BY` 경로 | `SORT BY name` | 그 필드로 오름차순 정렬합니다 |
| `SORT BY` 경로 `ASC` | `SORT BY name ASC` | 위와 같습니다. `ASC`는 써도 되고 빼도 됩니다 |
| `SORT BY` 경로 `DESC` | `SORT BY age DESC` | 그 필드로 내림차순 정렬합니다 |
| `SORT BY` 경로 여럿 | `SORT BY city, age DESC` | `city`로 정렬하고, city가 같은 객체는 `age` 내림차순으로 정렬합니다 |
| `LIMIT` 숫자 | `LIMIT 10` | 객체를 최대 그 개수까지 돌려줍니다 |
| `OFFSET` 숫자 | `OFFSET 20` | 그 개수만큼 먼저 건너뜁니다 |

- **순서.** 정렬 결과가 같은 객체는 기본 키 순서로 나오고, `SORT BY`가 없으면 모든 객체가 기본 키 순서로 나옵니다. null은 오름차순에서 맨 앞, 내림차순에서 맨 뒤에 옵니다.
- **목록**으로는 정렬할 수 없습니다. 객체 하나가 거기에 값을 여럿 가질 수 있기 때문이며, `INVALID_QUERY`로 실패합니다.
- **숫자.** `LIMIT`와 `OFFSET`에는 음수가 아닌 정수를 문자열에 직접 씁니다. 매개변수는 받지 않습니다.
- **페이지.** `SORT BY name LIMIT 20 OFFSET 40`은 스무 개씩 나눈 세 번째 페이지입니다. 개수 제한이 있고 엔진이 인덱스에서 바로 정렬 순서대로 읽을 수 있는 쿼리는 제한에 이르면 읽기를 멈춥니다.

## 필드와 경로

경로는 필드 하나를 가리키거나, `.`으로 내장 객체와 링크를 따라갑니다.

| 경로               | 예                         | 검사하는 것                                 |
| ------------------ | -------------------------- | ------------------------------------------- |
| 필드               | `age > 30`                 | 객체 자신의 필드                            |
| 내장 객체          | `address.city == "Lisbon"` | `address`에 든 내장 객체의 필드             |
| 링크               | `team.name == "Core"`      | 링크 `team`이 가리키는 객체의 필드          |
| 링크 자체          | `team == 3`                | 링크한 객체의 기본 키                       |
| 따옴표로 감싼 이름 | `` `limit` > 3 ``          | 이름이 키워드이거나 평범한 단어가 아닌 필드 |

- **이름**은 글자와 숫자, `_`로 쓰고 숫자로 시작하지 않습니다. 다른 이름은 백틱으로 감쌉니다. `` `first name` ``이나 `` `limit` ``처럼 키워드인 이름이 그렇습니다.
- **점 뒤의 키워드**는 이름으로 읽으므로 `team.limit`에는 백틱이 필요 없습니다. 키워드는 경로가 시작하는 자리에서만 백틱이 필요합니다.
- **없는 객체를 가리키는 링크**는 null로 읽습니다. 그래서 `team.name IS NULL`은 팀 이름이 없는 객체와 함께 팀이 없는 객체도 찾습니다.
- **길이.** 경로에는 이름이 최대 32개까지 들어갑니다. 조건은 내장 객체의 필드 하나를 검사하므로 경로가 내장 객체에서 끝날 수는 없습니다.

## 값

| 값       | 쓰는 법                                | 예                                |
| -------- | -------------------------------------- | --------------------------------- |
| 정수     | 숫자. 음수는 앞에 `-`를 붙입니다       | `42`, `-7`                        |
| 실수     | 소수점이나 지수, 또는 둘 다 있는 숫자  | `0.5`, `-1.25`, `6.02e23`, `1E-9` |
| 문자열   | 큰따옴표                               | `"Seoul"`, `"say \"hi\""`         |
| 불리언   | `true`, `false`                        | `active == true`                  |
| null     | `null`                                 | `email == null`                   |
| 매개변수 | `$`와 매개변수 번호                    | `$0`, `$1`                        |
| 바이트   | 리터럴이 없습니다. 매개변수로 넘깁니다 | `hash == $0`                      |

문자열에서 쓸 수 있는 이스케이프는 다섯 가지이며, 그 밖의 글자 앞에 백슬래시를 쓰면 `INVALID_QUERY`로 실패합니다.

| 이스케이프 | 뜻                                             |
| ---------- | ---------------------------------------------- |
| `\"`       | 큰따옴표                                       |
| `\\`       | 백슬래시                                       |
| `\n`       | 줄바꿈                                         |
| `\t`       | 탭                                             |
| `\u{...}`  | 그 16진수 코드의 글자. `\u{AC00}`은 `가`입니다 |

정수는 64비트 안에 들어가야 합니다. `true`와 `false`, `null`은 다른 키워드처럼 대소문자를 가리지 않습니다.

## 매개변수

`$0`, `$1` 같은 매개변수는 문자열과 함께 넘긴 값을 차례대로, 값의 타입 그대로 대신합니다. 같은 매개변수를 여러 번 써도 되고, 값을 쓰는 자리라면 어디에나 쓸 수 있습니다. `age BETWEEN $0 AND $1`이나 `city IN [$0, $1]`처럼 씁니다.

- **프로그램 밖에서 온 값**은 문자열에 넣지 말고 반드시 매개변수로 넘깁니다. 매개변수는 무엇이 들어 있든 값으로만 쓰이지만, 쿼리에 이어 붙인 문자열은 쿼리의 일부로 읽힙니다.
- **값이 모자라면**, 예를 들어 값 두 개를 주고 `$2`를 쓰면 `INVALID_QUERY`로 실패합니다.
- **한 번만 해석하기.** 자주 도는 쿼리는 [준비](./queries.md#자주-도는-쿼리-준비하기)해 두면 한 번만 해석하고 실행할 때마다 값만 받습니다. Node.js와 Dart, Python 패키지는 해석한 문자열을 256개까지 기억해 두므로, 같은 문자열을 다른 값으로 다시 실행해도 다시 해석하지 않습니다.

## 키워드

`AND`, `OR`, `NOT`, `BETWEEN`, `IN`, `CONTAINS`, `STARTSWITH`, `ENDSWITH`, `IS`, `NULL`, `TRUE`, `FALSE`, `SORT`, `BY`, `ASC`, `DESC`, `LIMIT`, `OFFSET`입니다. 대소문자를 가리지 않으므로 `sort by age desc`도 같은 쿼리이며, 이 이름을 가진 필드는 경로가 시작하는 자리에서 백틱으로 감쌉니다.

## 오류

해석할 수 없는 문자열은 `INVALID_QUERY`로 실패합니다. 메시지에는 1부터 센 글자 위치와 그 자리에 와야 했던 것이 나옵니다.

| 문자열               | 메시지                                                |
| -------------------- | ----------------------------------------------------- |
| `age >=`             | at character 7: expected a value, found the end       |
| `age >= 18 LIMIT -1` | at character 17: a limit or an offset is not negative |
| `name == "Ada`       | at character 9: a string does not end                 |
| `desc == 1`          | at character 1: expected a field name, found `desc`   |

해석은 되지만 컬렉션에 맞지 않는 쿼리도 실행할 때 `INVALID_QUERY`로 실패합니다. 컬렉션에 없는 필드를 쓰거나 필드를 다른 타입의 값과 비교하는 경우가 그렇습니다.

## 문법

```text
query       = [ filter ] [ "SORT" "BY" sort { "," sort } ] [ "LIMIT" int ] [ "OFFSET" int ]
sort        = path [ "ASC" | "DESC" ]
filter      = and { "OR" and }
and         = not { "AND" not }
not         = "NOT" not | "(" filter ")" | condition
condition   = path compare value
            | path "BETWEEN" value "AND" value
            | path "IN" "[" [ value { "," value } ] "]"
            | path ( "CONTAINS" | "STARTSWITH" | "ENDSWITH" ) value
            | path "IS" [ "NOT" ] "NULL"
compare     = "==" | "!=" | "<" | "<=" | ">" | ">="
path        = name { "." name }
value       = int | float | string | "true" | "false" | "null" | "$" digits
```

## 예제

아래 예제는 다음 필드를 가진 사용자 컬렉션에서 실행합니다. `name`과 `city`는 문자열, `email`은 고유 인덱스가 있는 선택 문자열, `age`는 인덱스가 있는 정수, `tags`는 문자열 목록, `address`는 `city`와 `zip`을 가진 내장 객체, `team`은 `name`을 가진 팀 컬렉션으로 가는 링크입니다.

| 쿼리 | 찾는 객체 |
| --- | --- |
| `email == "ada@example.com"` | 이 이메일을 가진 사용자 한 명. 고유 인덱스에서 찾습니다 |
| `age >= 18 AND age < 30` | 18세부터 29세까지의 사용자. `age` 인덱스에서 읽습니다 |
| `age BETWEEN 18 AND 29` | 위와 같은 사용자 |
| `city IN ["Seoul", "Busan"]` | 두 도시 가운데 한 곳에 사는 사용자 |
| `(city == "Seoul" OR city == "Busan") AND age >= 18` | 두 도시 가운데 한 곳에 사는 성인 |
| `name STARTSWITH "Ma" SORT BY name` | 이름이 `Ma`로 시작하는 사용자를 이름순으로 |
| `email ENDSWITH "@example.com"` | 그 도메인의 주소를 가진 사용자 |
| `tags CONTAINS "admin"` | 태그 `admin`이 있는 사용자 |
| `tags STARTSWITH "team-"` | `team-`으로 시작하는 태그가 있는 사용자 |
| `email IS NULL` | 이메일이 없는 사용자 |
| `city != "Seoul"` | 서울이 아닌 도시에 사는 사용자 |
| `NOT city == "Seoul"` | 위의 사용자와 도시가 없는 사용자 |
| `address.city == "Lisbon"` | 내장 주소가 리스본인 사용자 |
| `team.name == "Core"` | 링크한 팀의 이름이 Core인 사용자 |
| `team == 3` | 키가 3인 팀에 링크된 사용자 |
| `SORT BY age DESC, name LIMIT 10` | 나이가 많은 사용자 열 명. 나이가 같으면 이름순입니다 |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | 서울에 사는 사용자를 스무 명씩 나눈 세 번째 페이지 |
| `age >= $0 AND city == $1` | 어떤 나이 이상이고 어떤 도시에 사는 사용자. 둘 다 값으로 넘깁니다 |
