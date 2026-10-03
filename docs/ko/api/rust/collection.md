---
title: Collection
order: 8
---

# Collection

`Collection`은 스키마의 컬렉션 하나를 선언합니다. 필드와 기본 키, 인덱스가 여기에 들어갑니다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Collection
```

`Collection::new`로 시작하고, 메서드마다 필드나 인덱스를 하나 더해 컬렉션을 돌려주며, [`Schema::collection`](./schema.md#collection)으로 스키마에 넣습니다. 필드 타입은 [`Type`](../../types/rust/type.md)입니다. [`primary_key`](#primary-key)를 주지 않으면, 엔진이 차례로 번호를 매기는 `id`라는 `Int` 필드가 기본 키가 됩니다.

```rust
use darudb::{Collection, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}
```

선언은 이 컬렉션으로 파일을 열 때 검사합니다. 다음은 모두 `open`이 `INVALID_ARGUMENT`로 실패합니다.

- **이름.** 이름이 빈 컬렉션이나 필드, 이름이 같은 필드 둘. 기본 키가 없는 컬렉션의 `id` 필드. `id`는 자동 증가 키가 쓰는 이름입니다.
- **기본 키.** `Int`, `String`, `Bytes`가 아닌 기본 키.
- **타입.** 스키마에 없는 컬렉션을 가리키는 링크, 목록의 목록이나 내장 객체의 목록.
- **기본값.** 필드의 타입과 맞지 않는 기본값, 링크나 내장 객체에 준 기본값.
- **인덱스.** 컬렉션에 없는 필드, 내장 객체나 내장 객체의 목록에 건 인덱스, 같은 필드에 두 번 건 인덱스.

## 연관 함수

### new

```rust
pub fn new(name: impl Into<String>) -> Self
```

이름이 `name`이고 필드가 아직 없는 컬렉션을 만듭니다. 기본 키는 자동 증가 키입니다.

## 메서드

### primary_key

```rust
pub fn primary_key(mut self, name: impl Into<String>, kind: Type) -> Self
```

`kind` 타입의 `name` 필드를 기본 키로 더합니다. 타입은 `Int`, `String`, `Bytes` 중 하나입니다. 기본 키는 필수이며, 나중에 마이그레이션으로 바꿀 수 없습니다.

### field

```rust
pub fn field(mut self, name: impl Into<String>, kind: Type) -> Self
```

필수 필드를 더합니다. 이 필드가 빠진 객체를 쓰면 `INVALID_ARGUMENT`로 실패합니다.

### optional

```rust
pub fn optional(mut self, name: impl Into<String>, kind: Type) -> Self
```

선택 필드를 더합니다. null일 수 있고, 객체에서 빠지면 null이 됩니다.

### with_default

```rust
pub fn with_default(
    mut self,
    name: impl Into<String>,
    kind: Type,
    value: impl Into<Value>,
) -> Self
```

객체에서 빠지면 `value`가 들어가는 필수 필드를 더합니다. 기본값은 필드 타입의 스칼라나 스칼라의 목록이어야 합니다. 필드가 생기기 전에 쓴 객체는 그 필드의 기본값을 읽으므로, 한번 기본값을 준 필드는 이후 버전의 스키마에서도 기본값을 유지해야 합니다.

### index

```rust
pub fn index(mut self, name: impl Into<String>) -> Self
```

`name` 필드에 인덱스를 둡니다. 그 필드에 조건을 걸거나 그 필드 하나로만 정렬하는 쿼리는 모든 객체 대신 인덱스를 읽습니다. 인덱스를 둘 수 있는 필드는 스칼라나 링크, 또는 그 목록이며, 목록 필드의 인덱스에는 원소마다 항목이 생깁니다.

### unique

```rust
pub fn unique(mut self, name: impl Into<String>) -> Self
```

`name` 필드에 인덱스를 두고 값이 겹치지 않게 합니다. 같은 값을 가진 객체는 둘일 수 없지만, null은 몇 개가 있어도 됩니다. 다른 객체가 가진 값을 쓰려는 쓰기는 `DUPLICATE_KEY`로 실패합니다. 새 버전에서 고유 인덱스를 더했는데 파일의 객체에 같은 값이 둘 있으면, 파일을 열 때도 `DUPLICATE_KEY`로 실패합니다.
