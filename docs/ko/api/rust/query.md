---
title: Query
order: 13
---

# Query

`Query`는 컬렉션에서 어떤 객체를 어떤 순서로 몇 개 찾을지 적습니다.

```rust
#[derive(Debug, Clone, Default)]
pub struct Query
```

`Query::new`에서 시작해 메서드로 만들거나, [`parse`](#parse)나 [`prepare`](#prepare)로 문자열을 파싱해 만듭니다. 어느 쪽이든 같은 쿼리가 됩니다. 쿼리는 컬렉션이나 트랜잭션에 묶이지 않으므로 하나를 여러 번 실행할 수 있습니다. [`CollectionReader::query`](./collection-reader.md#query)와 [`count`](./collection-reader.md#count)가 쿼리를 실행하면서 컬렉션의 스키마와 맞는지 검사합니다. 없는 필드를 쓰거나 타입이 틀린 값을 쓰면 그때 `INVALID_QUERY`로 실패합니다.

정렬을 주지 않으면 기본 키 순서로 나오고, 정렬 값이 같은 객체끼리도 기본 키 순서를 따릅니다. 두 쿼리가 같은 객체를 같은 방법으로 찾으면 서로 같은 쿼리로 봅니다. 그래서 값을 채운 준비한 쿼리는 같은 값을 직접 적은 쿼리와 같습니다. `Query`는 `Send`이자 `Sync`입니다.

```rust
use darudb::{Database, Filter, Query};

fn adults(db: &Database) -> darudb::Result<()> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);

    for user in users.query(&query)? {
        println!("{:?}", user.get("name"));
    }

    Ok(())
}
```

기본 키나 인덱스가 있는 필드에 건 조건이 나머지 필터와 `and`로 이어져 있으면, 엔진은 그 조건에 맞는 객체만 읽습니다. 인덱스가 있는 필드 하나로만 정렬하면 그 순서대로 읽다가 개수 제한에서 멈춥니다. 그렇지 않으면 컬렉션의 객체를 모두 읽습니다. 자세한 내용은 [쿼리](../../guide/queries.md)에 있습니다.

## 연관 함수

### new

```rust
pub fn new() -> Self
```

컬렉션의 모든 객체를 기본 키 순서로 찾는 쿼리입니다. `Query::default()`와 같습니다.

### parse

```rust
pub fn parse(text: &str, parameters: &[Value]) -> Result<Self>
```

쿼리 언어로 쓴 `text`를 파싱하고, `$0`, `$1` 같은 매개변수에 `parameters`의 값을 넣습니다. 필터를 먼저 쓰고 `SORT BY`, `LIMIT`, `OFFSET`을 차례로 붙이며, 모두 생략할 수 있습니다. 문법에 맞지 않거나 `parameters`에 없는 매개변수를 쓴 문자열은 `INVALID_QUERY`로 실패하고, 메시지에 문제가 생긴 글자의 위치가 나옵니다.

```rust
use darudb::Query;

fn main() -> darudb::Result<()> {
    let query = Query::parse(
        r#"age >= $0 AND (name STARTSWITH "A" OR tags CONTAINS "admin")
           SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;
    let _ = query;

    Ok(())
}
```

[`Filter`](./filter.md)의 조건은 문자열로도 쓸 수 있습니다.

| 텍스트                                   | Filter                                 |
| ---------------------------------------- | -------------------------------------- |
| `f == v`, `f != v`                       | `eq`, `ne`                             |
| `f < v`, `f <= v`, `f > v`, `f >= v`     | `lt`, `le`, `gt`, `ge`                 |
| `f BETWEEN v AND w`                      | `between`                              |
| `f IN [v, w]`                            | `is_in`                                |
| `f CONTAINS v`, `STARTSWITH`, `ENDSWITH` | `contains`, `starts_with`, `ends_with` |
| `f IS NULL`, `f IS NOT NULL`             | `is_null`, `is_not_null`               |
| `a AND b`, `a OR b`, `NOT a`, `(a)`      | `and`, `or`, `!`                       |

- **우선순위.** `NOT`이 가장 먼저 묶이고, 그다음이 `AND`, 마지막이 `OR`입니다. 괄호로 묶을 수 있습니다.
- **정렬.** `SORT BY f DESC, g`처럼 경로 여러 개로 정렬합니다. 뒤에 `DESC`가 없으면 오름차순입니다. `LIMIT`과 `OFFSET`에는 음수가 아닌 정수를 씁니다.
- **이름.** 필드는 단어로 쓰고, 경로는 이름을 `.`으로 잇습니다. `` `limit` ``처럼 키워드와 같거나 평범한 단어가 아닌 이름은 백틱으로 감쌉니다. 키워드는 대소문자를 가리지 않습니다.
- **값.** 정수, 소수점이나 지수가 있는 실수, 큰따옴표로 감싼 문자열, `true`, `false`, `null`을 씁니다. 문자열 안에서는 `\"`, `\\`, `\n`, `\t`, `\u{...}`로 이스케이프합니다. 바이트에는 리터럴이 없으므로 매개변수로 넘깁니다.
- **매개변수.** `$0`, `$1` 같은 매개변수에는 문자열과 함께 넘긴 값이 순서대로 들어갑니다. 애플리케이션 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요.

### prepare

```rust
pub fn prepare(text: &str) -> Result<Self>
```

`parse`처럼 `text`를 파싱하되, `$0`, `$1` 같은 매개변수를 비워 둡니다. 값만 바꿔 여러 번 실행할 쿼리에 씁니다. [`bind`](#bind)는 문자열을 다시 파싱하지 않고 값을 채웁니다. 값을 채우지 않은 준비한 쿼리를 실행하면 `INVALID_QUERY`로 실패합니다.

```rust
use darudb::Query;

fn main() -> darudb::Result<()> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["alice@example.com".into()])?;
    let _ = query;

    Ok(())
}
```

## 메서드

### filter

```rust
pub fn filter(mut self, filter: Filter) -> Self
```

`filter`에 맞는 객체만 남깁니다. 앞서 준 필터가 있으면 둘 다 맞아야 합니다. 두 번 부르면 `and`로 이어집니다.

### sort_by

```rust
pub fn sort_by(mut self, field: &str) -> Self
```

`field`로 오름차순 정렬합니다. `field`는 이름이나 경로이고, 앞서 준 정렬이 있으면 그 뒤에 붙습니다. null은 맨 앞에 오고, 문자열은 바이트 순서로 비교합니다. 목록처럼 값을 여러 개 가진 필드로는 정렬할 수 없으며, 쿼리를 실행할 때 `INVALID_QUERY`로 실패합니다.

### sort_by_desc

```rust
pub fn sort_by_desc(mut self, field: &str) -> Self
```

`field`로 내림차순 정렬하고, 앞서 준 정렬이 있으면 그 뒤에 붙습니다. null은 맨 뒤에 옵니다.

### offset

```rust
pub fn offset(mut self, count: u64) -> Self
```

결과에서 처음 `count`개를 건너뜁니다. 다시 부르면 앞의 값을 바꿉니다.

### limit

```rust
pub fn limit(mut self, count: u64) -> Self
```

객체를 많아야 `count`개 돌려줍니다. 다시 부르면 앞의 값을 바꿉니다.

### first

```rust
pub fn first(mut self) -> Self
```

많아야 첫 객체 하나만 돌려줍니다. 개수 제한이 1이 되고, 쿼리에 이미 준 제한이 0이면 0이 됩니다. 객체 하나를 찾는 조회는 거기서 읽기를 멈춥니다.

### bind

```rust
pub fn bind(&self, parameters: &[Value]) -> Result<Self>
```

준비한 쿼리의 `$0`, `$1` 같은 매개변수에 `parameters`의 값을 채운 쿼리를 돌려줍니다. 값을 채운 쿼리는 준비한 쿼리의 파싱 결과를 복사하지 않고 함께 씁니다. 쿼리가 쓰는 매개변수보다 값이 적으면 `INVALID_QUERY`로 실패합니다. 매개변수가 없는 쿼리나 이미 값을 채운 쿼리는 그대로 돌아옵니다.

### bind_encoded

```rust
pub fn bind_encoded(&self, parameters: &[u8]) -> Result<Self>
```

언어 바인딩이 버퍼 하나에 담아 보낸 매개변수로 `bind`합니다. 버퍼는 필드 0에 매개변수 개수가, 필드 `n + 1`에 매개변수 `n`의 값이 든 레코드이며, null인 매개변수는 빠집니다. Rust 프로그램은 `bind`를 씁니다. 인코딩 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-ir)에 있습니다.
