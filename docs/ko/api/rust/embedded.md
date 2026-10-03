---
title: Embedded
order: 11
counterpart: /api/node/t
---

# Embedded

`Embedded`는 내장 객체의 필드를 선언합니다. 내장 객체는 다른 객체 안에 저장되며 자기 필드를 가진 객체입니다.

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Embedded
```

`Embedded::new`로 시작하고, 메서드마다 필드를 하나 더해 돌려주며, [`Type::object`](../../types/rust/type.md)로 필드의 타입으로 씁니다. 필드 규칙은 [`Collection`](./collection.md)의 필드와 같고, 링크와 스칼라의 목록, 다른 내장 객체도 필드로 가질 수 있습니다. 내장 객체에는 기본 키도 인덱스도 없지만, 쿼리는 `address.city` 같은 경로로 그 필드에 닿습니다.

타입이 내장 객체인 필드는 `field`나 `optional`로 선언합니다. 이 필드 자체에는 기본값을 줄 수 없고, 기본값은 내장 객체의 필드마다 줍니다. [`CollectionWriter::update`](./collection-writer.md#update)는 내장 객체를 통째로 바꿉니다.

```rust
use darudb::{Collection, Embedded, Schema, Type};

fn schema() -> Schema {
    let address = Embedded::new()
        .field("city", Type::String)
        .optional("street", Type::String)
        .with_default("floor", Type::Int, 0);

    Schema::new(1).collection(
        Collection::new("places")
            .primary_key("code", Type::String)
            .field("name", Type::String)
            .optional("address", Type::object(address)),
    )
}
```

## 연관 함수

### of

```rust
pub fn of<E: EmbeddedType>() -> Self
```

`E`가 선언하는 필드를 돌려줍니다. [`#[derive(Embedded)]`](./derive.md#embedded)가 선언한 그대로입니다. Rust 타입이 `E`인 필드는 이 선언을 스스로 하므로 직접 부를 일은 드뭅니다.

### new

```rust
pub fn new() -> Self
```

필드가 아직 없는 내장 객체를 만듭니다. `Embedded::default()`와 같습니다.

## 메서드

### field

```rust
pub fn field(mut self, name: impl Into<String>, kind: Type) -> Self
```

필수 필드를 더합니다.

### optional

```rust
pub fn optional(mut self, name: impl Into<String>, kind: Type) -> Self
```

null일 수 있는 선택 필드를 더합니다.

### with_default

```rust
pub fn with_default(
    mut self,
    name: impl Into<String>,
    kind: Type,
    value: impl Into<Value>,
) -> Self
```

내장 객체에서 빠지면 `value`가 들어가는 필수 필드를 더합니다.
