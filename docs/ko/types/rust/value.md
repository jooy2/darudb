---
title: Value
order: 2
group: objects
pageClass: reference-page
---

# Value

`Value`는 객체의 필드 하나에 든 값이며, 기본 키와 쿼리가 필드와 비교하는 값도 이 타입으로 나타냅니다.

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value
```

[`Object`](./object.md)는 필드마다 `Value`를 하나씩 담습니다. [`get`](../../api/rust/collection-reader.md)이나 [`delete`](../../api/rust/collection-writer.md)처럼 키를 받는 메서드는 `impl Into<Value>`를 받으므로 키를 `42`나 `"alice"`처럼 넘기면 되고, `insert`는 저장한 키를 `Value`로 돌려줍니다. [`Filter`](../../api/rust/filter.md)도 값을 같은 방식으로 받고, [`Query::parse`](../../api/rust/query.md)는 매개변수를 `&[Value]`로 받습니다. 값에는 필드 타입이 따로 붙지 않아서, `Int`가 숫자인지 링크인지는 스키마가 정합니다. 날짜는 `Date`이며, [`TryFrom<SystemTime>`](#tryfrom-systemtime)으로 시각에서 만듭니다. 릴리스마다 배리언트가 늘 수 있으므로 `Value`에 쓰는 `match`에는 와일드카드 갈래가 있어야 합니다.

## 배리언트

| 배리언트           | 담는 값                                                          |
| ------------------ | ---------------------------------------------------------------- |
| `Null`             | 값 없음. 빠진 선택 필드                                          |
| `Bool(bool)`       | `false` 또는 `true`                                              |
| `Int(i64)`         | 부호 있는 64비트 정수                                            |
| `Float(f64)`       | 64비트 부동소수점 수                                             |
| `String(String)`   | UTF-8 텍스트                                                     |
| `Bytes(Vec<u8>)`   | 임의의 바이트                                                    |
| `List(Vec<Value>)` | 한 타입의 값으로 된 목록                                         |
| `Object(Object)`   | 내장 객체                                                        |
| `Date(i64)`        | 시각. 1970-01-01T00:00:00 UTC부터의 마이크로초이며, 그 전은 음수 |

## 필드 타입

필드가 가질 수 있는 [`Type`](./type.md)마다 받는 배리언트가 하나씩 정해져 있습니다.

| 필드 타입      | 값                                                                              |
| -------------- | ------------------------------------------------------------------------------- |
| `Type::Bool`   | `Bool`                                                                          |
| `Type::Int`    | `Int`                                                                           |
| `Type::Float`  | `Float`                                                                         |
| `Type::String` | `String`                                                                        |
| `Type::Bytes`  | `Bytes`                                                                         |
| `Type::Date`   | `Date`                                                                          |
| `Type::Link`   | 가리키는 객체의 기본 키. 그 컬렉션의 키에 따라 `Int`, `String`, `Bytes` 중 하나 |
| `Type::List`   | `List`. 원소마다 원소 타입의 배리언트                                           |
| `Type::Object` | `Object`. 그 필드에도 같은 규칙이 적용됨                                        |

쓰기는 모든 값을 스키마와 대조합니다. 맞지 않는 값이 하나라도 있으면 아무것도 바꾸지 않고 `INVALID_ARGUMENT`로 실패합니다.

- **배리언트가 정확히 맞아야 합니다.** `Float` 필드에 `Int`를 넣을 수 없고, `Int` 필드에 `Float`를 넣을 수도 없습니다.
- **`Null`.** 선택 필드에 `Null`을 주면 필드를 뺀 것과 같습니다. 필수 필드에 `Null`을 주거나 필드를 빼면 기본값이 들어가고, 기본값이 없으면 실패합니다. 다만 자동 증가 키를 쓰는 컬렉션의 `id`는 다음 번호를 받습니다. 목록의 원소로는 `Null`을 넣을 수 없습니다.
- **링크.** 가리키는 컬렉션의 키 타입과 맞는지만 확인하고, 그 객체가 있는지는 확인하지 않습니다. 자동 증가 키를 쓰는 컬렉션의 키는 `Int`입니다.
- **필드가 아닌 이름.** 컬렉션에 없는 이름이든 내장 객체에 없는 이름이든 거부합니다.

읽을 때는 같은 배리언트로 돌아옵니다. 값이 없는 선택 필드는 `Null`로 읽히고, 저장된 객체보다 나중에 생긴 필드는 기본값이나 `Null`로 읽힙니다.

`Int`가 `Float`를 대신하는 곳은 쿼리뿐입니다. `Float` 필드와 비교할 때 -2^53부터 2^53까지의 정수는 그와 같은 부동소수점 수로 봅니다. 숫자 타입이 하나뿐인 언어에서는 `1`과 `1.0`을 구별할 수 없기 때문입니다. 값을 비교하는 나머지 규칙은 [쿼리](../../guide/queries.md)에 있습니다.

## 메서드

| 메서드                    | 반환                 | 값이 이것일 때 |
| ------------------------- | -------------------- | -------------- |
| [`is_null`](#is-null)     | `bool`               | `Null`         |
| [`as_bool`](#as-bool)     | `Option<bool>`       | `Bool`         |
| [`as_int`](#as-int)       | `Option<i64>`        | `Int`          |
| [`as_float`](#as-float)   | `Option<f64>`        | `Float`        |
| [`as_str`](#as-str)       | `Option<&str>`       | `String`       |
| [`as_bytes`](#as-bytes)   | `Option<&[u8]>`      | `Bytes`        |
| [`as_date`](#as-date)     | `Option<i64>`        | `Date`         |
| [`as_time`](#as-time)     | `Option<SystemTime>` | `Date`         |
| [`as_list`](#as-list)     | `Option<&[Value]>`   | `List`         |
| [`as_object`](#as-object) | `Option<&Object>`    | `Object`       |

아래 메서드는 값을 변환하지 않습니다. `Int`에 `as_float`를 부르면 `None`이 나옵니다.

### is_null

```rust
pub fn is_null(&self) -> bool
```

값이 `Value::Null`인지 알려 줍니다.

### as_bool

```rust
pub fn as_bool(&self) -> Option<bool>
```

값이 `Bool`이면 그 불리언을 돌려줍니다.

### as_int

```rust
pub fn as_int(&self) -> Option<i64>
```

값이 `Int`이면 그 정수를 돌려줍니다.

### as_float

```rust
pub fn as_float(&self) -> Option<f64>
```

값이 `Float`이면 그 수를 돌려줍니다.

### as_str

```rust
pub fn as_str(&self) -> Option<&str>
```

값이 `String`이면 그 텍스트를 돌려줍니다.

### as_bytes

```rust
pub fn as_bytes(&self) -> Option<&[u8]>
```

값이 `Bytes`이면 그 바이트를 돌려줍니다.

### as_list

```rust
pub fn as_list(&self) -> Option<&[Value]>
```

값이 `List`이면 그 원소를 돌려줍니다.

#### as_date

```rust
pub fn as_date(&self) -> Option<i64>
```

값이 `Date`라면 1970-01-01T00:00:00 UTC부터의 마이크로초입니다.

### as_time

```rust
pub fn as_time(&self) -> Option<SystemTime>
```

값이 플랫폼의 `SystemTime`이 담을 수 있는 `Date`라면 그 날짜를 `SystemTime`으로 돌려줍니다.

## `TryFrom<SystemTime>`

```rust
impl TryFrom<SystemTime> for Value
```

시각을 마이크로초 단위로 내림한 날짜입니다. 그래서 1970년 이전의 시각에 마이크로초보다 작은 부분이 있으면 바로 앞 마이크로초가 됩니다. 1970년에서 약 29만 2천 년보다 먼 시각은 `INVALID_ARGUMENT`로 실패합니다. `Value::try_from(SystemTime::now())?`처럼 씁니다.

## as_object

```rust
pub fn as_object(&self) -> Option<&Object>
```

값이 `Object`이면 그 내장 객체를 돌려줍니다.

## 변환

`Value`는 아래 타입에서 `From`으로 변환됩니다. 그래서 `Object::with`와 `Object::set`, 그리고 `impl Into<Value>`를 받는 모든 메서드에 Rust 값을 그대로 넘길 수 있습니다.

| 원래 타입                     | 배리언트                                 |
| ----------------------------- | ---------------------------------------- |
| `bool`                        | `Bool`                                   |
| `i64`, `i32`, `u32`           | `Int`                                    |
| `f64`                         | `Float`                                  |
| `&str`, `String`              | `String`                                 |
| `&[u8]`, `Vec<u8>`            | `Bytes`                                  |
| `Vec<Value>`                  | `List`                                   |
| `Object`                      | `Object`                                 |
| `Option<T>`, `T: Into<Value>` | `None`이면 `Null`, 아니면 `T`의 배리언트 |

- `u64`, `usize`, `f32`에서는 변환되지 않습니다. `u64`는 `Int`의 범위를 넘을 수 있으니 먼저 `i64::try_from`으로 바꿔야 합니다.
- `b"abc"` 같은 바이트 문자열 리터럴은 슬라이스가 아니라 배열입니다. `b"abc".as_slice()`나 `b"abc".to_vec()`으로 넘깁니다.
- 목록은 값으로 만듭니다. `vec!["a", "b"]`가 아니라 `vec![Value::from("a"), Value::from("b")]`로 씁니다.

```rust
use darudb::{Object, Value};

let post = Object::new()
    .with("title", "Hello")
    .with("views", 0)
    .with("rating", 4.5)
    .with("cover", b"\x89PNG".to_vec())
    .with("subtitle", None::<String>)
    .with("tags", vec![Value::from("intro"), Value::from("rust")]);
```
