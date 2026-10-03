---
title: Filter
order: 11
counterpart: /api/node/conditions
---

# Filter

`Filter`는 [`Query::filter`](./query.md#filter)에 주는 조건으로, 객체가 맞아야 할 조건을 나타냅니다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Filter
```

연관 함수는 필드 하나에 거는 조건을 만들고, `and`, `or`, `!`로 조건을 엮습니다. 필터를 만드는 일은 실패하지 않습니다. 쿼리를 실행할 때 컬렉션의 스키마와 맞는지 검사하며, 없는 필드를 쓰거나 타입이 틀린 값을 쓰면 그때 `INVALID_QUERY`로 실패합니다. 조건마다 쿼리 언어로 쓰는 형태가 있으며, [`Query::parse`](./query.md#parse)에 정리돼 있습니다.

```rust
use darudb::Filter;

fn main() {
    let adults_named_a = Filter::ge("age", 18).and(Filter::starts_with("name", "A"));
    let not_in_seoul = !Filter::eq("address.city", "Seoul");
    let tagged = Filter::is_in("tags", ["intro", "news"]).or(Filter::is_null("tags"));
    let _ = (adults_named_a, not_in_seoul, tagged);
}
```

## 경로

조건의 `field`는 경로입니다. 필드 이름 하나를 쓰거나, 내장 객체나 링크를 지날 때는 이름을 `.`으로 잇습니다.

- **내장 객체.** `address.city`는 `address`에 든 내장 객체의 필드를 검사합니다. 조건은 내장 객체의 필드 하나를 검사하므로, 경로가 내장 객체에서 끝날 수는 없습니다.
- **링크.** `author.name`은 링크 `author`가 가리키는 객체의 필드를 검사하고, `author`만 쓰면 가리키는 객체의 기본 키를 검사합니다. 가리키는 객체가 없으면 null로 읽습니다.
- **목록.** 목록에 건 조건은 원소 하나라도 맞으면 참입니다. 목록에 `contains`를 쓰면 그 원소가 있는지 봅니다. 빈 목록은 null이 아닙니다.
- **길이.** 경로에는 이름을 32개까지 쓸 수 있습니다.

## 값

값은 필드의 타입과 같아야 합니다. `Int` 필드는 정수와 비교하고 실수와는 비교하지 않습니다. `Float` 필드는 정수와 실수 모두와 비교합니다. 링크는 대상 컬렉션의 키와 비교합니다. null인 필드에 건 조건은 `is_null`을 빼고 모두 거짓입니다. 문자열은 바이트 순서로 비교합니다.

## 연관 함수

### eq

```rust
pub fn eq(field: &str, value: impl Into<Value>) -> Self
```

필드가 `value`와 같습니다. `Value::Null`과 같다는 조건은 [`is_null`](#is-null)입니다.

### ne

```rust
pub fn ne(field: &str, value: impl Into<Value>) -> Self
```

필드가 null이 아니고 `value`와 다릅니다. `Value::Null`과 다르다는 조건은 [`is_not_null`](#is-not-null)입니다.

### lt

```rust
pub fn lt(field: &str, value: impl Into<Value>) -> Self
```

필드가 `value`보다 작습니다.

### le

```rust
pub fn le(field: &str, value: impl Into<Value>) -> Self
```

필드가 `value` 이하입니다.

### gt

```rust
pub fn gt(field: &str, value: impl Into<Value>) -> Self
```

필드가 `value`보다 큽니다.

### ge

```rust
pub fn ge(field: &str, value: impl Into<Value>) -> Self
```

필드가 `value` 이상입니다.

### between

```rust
pub fn between(field: &str, low: impl Into<Value>, high: impl Into<Value>) -> Self
```

필드가 `low`부터 `high`까지의 범위에 있습니다. 양 끝도 범위에 듭니다.

### is_in

```rust
pub fn is_in<V: Into<Value>>(field: &str, values: impl IntoIterator<Item = V>) -> Self
```

필드가 `values` 중 하나와 같습니다.

### contains

```rust
pub fn contains(field: &str, value: impl Into<Value>) -> Self
```

문자열 필드가 `value`를 포함하거나, 목록 필드가 `value`를 원소로 가집니다.

### starts_with

```rust
pub fn starts_with(field: &str, value: impl Into<Value>) -> Self
```

문자열 필드가 `value`로 시작합니다.

### ends_with

```rust
pub fn ends_with(field: &str, value: impl Into<Value>) -> Self
```

문자열 필드가 `value`로 끝납니다.

### is_null

```rust
pub fn is_null(field: &str) -> Self
```

필드가 null입니다.

### is_not_null

```rust
pub fn is_not_null(field: &str) -> Self
```

필드가 null이 아닙니다. `!Filter::is_null(field)`와 같습니다.

## 메서드

### and

```rust
pub fn and(self, other: Filter) -> Self
```

이 조건과 `other`가 모두 맞습니다.

### or

```rust
pub fn or(self, other: Filter) -> Self
```

이 조건과 `other` 중 하나 이상이 맞습니다.

### not

```rust
fn not(self) -> Filter
```

조건이 맞지 않습니다. `Not` 연산자의 구현이므로 `!filter`로 씁니다.
