---
title: Type
order: 3
counterpart: /api/node/t
---

# Type

`Type`은 필드의 타입으로, 스키마는 컬렉션과 내장 객체의 필드마다 타입을 하나씩 정합니다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Type
```

필드를 선언할 때 필드 이름과 함께 [`Collection`](../../api/rust/collection.md)이나 [`Embedded`](../../api/rust/embedded.md)에 넘깁니다. 스칼라 타입은 `Type::String`처럼 배리언트로 쓰고, 나머지는 `Type::link("users")`처럼 아래 연관 함수로 만듭니다. 타입마다 어떤 값을 담는지는 [Value](./value.md#필드-타입)에 있습니다.

```rust
use darudb::{Collection, Embedded, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .primary_key("handle", Type::String)
                .field("name", Type::String)
                .with_default("age", Type::Int, 0),
        )
        .collection(
            Collection::new("posts")
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .optional(
                    "place",
                    Type::object(Embedded::new().field("city", Type::String).optional("zip", Type::String)),
                )
                .index("author")
                .index("tags"),
        )
}
```

## 배리언트

| 배리언트           | 이 타입의 필드에 담는 값              |
| ------------------ | ------------------------------------- |
| `Bool`             | `false` 또는 `true`                   |
| `Int`              | 부호 있는 64비트 정수                 |
| `Float`            | 64비트 부동소수점 수                  |
| `String`           | UTF-8 텍스트                          |
| `Bytes`            | 임의의 바이트                         |
| `Link(String)`     | 지정한 컬렉션에 있는 객체의 기본 키   |
| `List(Box<Type>)`  | 스칼라 타입이나 링크의 값으로 된 목록 |
| `Object(Embedded)` | 자기 필드를 가진 내장 객체            |

## 연관 함수

### link

```rust
pub fn link(collection: impl Into<String>) -> Self
```

`collection`의 객체를 가리키는 링크입니다. `collection`은 같은 스키마에 있는 컬렉션이어야 합니다. 필드에는 그 객체의 기본 키가 들어가며, 그 객체가 실제로 없어도 됩니다.

### list

```rust
pub fn list(element: Type) -> Self
```

`element`의 값으로 된 목록입니다. `element`는 스칼라 타입이나 링크여야 하며, 목록의 목록이나 내장 객체의 목록은 거부합니다.

### object

```rust
pub fn object(embedded: Embedded) -> Self
```

`embedded`의 필드를 가진 내장 객체입니다. 바깥 객체의 레코드 안에 함께 저장되고, 자기 키는 없습니다.

## 타입을 쓸 수 있는 곳

스키마는 그 스키마로 데이터베이스를 열 때 검사합니다. 규칙을 어기면 열기가 `INVALID_ARGUMENT`로 실패합니다.

- **기본 키.** `Collection::primary_key`로 지정하는 필드는 `Int`, `String`, `Bytes` 중 하나입니다. 기본 키를 지정하지 않은 컬렉션에는 `id`라는 `Int` 필드가 생기고, 번호는 엔진이 매깁니다.
- **인덱스.** 스칼라 필드와 링크에 인덱스를 둘 수 있고, 이들의 목록에도 둘 수 있습니다. 목록의 인덱스에는 원소마다 항목이 생깁니다. 내장 객체에는 인덱스를 둘 수 없습니다.
- **기본값.** `with_default`에 주는 값은 필드 타입의 값이어야 하고, 목록 필드라면 그런 값의 목록이어야 합니다. 링크에는 기본값을 줄 수 없고, 내장 객체는 기본값 대신 자기 필드의 기본값을 씁니다.
- **링크 대상.** 링크가 가리키는 컬렉션은 같은 스키마에 선언돼 있어야 합니다.
