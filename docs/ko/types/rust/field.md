---
title: Field
order: 14
group: objects
counterpart: /types/node/stored-schema
pageClass: reference-page
---

# Field

`Field`는 컬렉션이나 내장 객체의 필드를 스키마가 선언한 모습 그대로 다시 읽은 것입니다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Field
```

필드는 [`Collection`](../../api/rust/collection.md)과 [`Embedded`](../../api/rust/embedded.md)의 빌더가 만들고, 두 타입의 `fields` 메서드가 다시 돌려줍니다. 직접 선언하지 않은 스키마를 살펴볼 때 읽습니다. 스키마 없이 연 파일에서 [`Database::stored_schema`](../../api/rust/database.md#stored-schema)가 돌려준 스키마가 그런 예입니다.

```rust
use darudb::{Database, Type};

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;

    for collection in db.stored_schema().iter().flat_map(|schema| schema.collections()) {
        for field in collection.fields() {
            let optional = if field.is_optional() { " (optional)" } else { "" };

            println!("{}.{}: {:?}{optional}", collection.name(), field.name(), field.kind());

            if let Type::Object(embedded) = field.kind() {
                println!("  {} fields of its own", embedded.fields().len());
            }
        }
    }

    Ok(())
}
```

## 메서드

### name

```rust
pub fn name(&self) -> &str
```

필드의 이름입니다.

### kind

```rust
pub fn kind(&self) -> &Type
```

필드의 [`Type`](./type.md)입니다. 내장 객체의 필드들은 `Type::Object`에 든 [`Embedded`](../../api/rust/embedded.md#fields)에 있습니다.

### is_optional

```rust
pub fn is_optional(&self) -> bool
```

필드가 null일 수 있는지 알려 줍니다. 객체에서 빠지면 null이 됩니다.

### is_cascade

```rust
pub fn is_cascade(&self) -> bool
```

링크가 가리키는 객체를 지울 때 그 링크를 가진 객체도 지우는지 알려 줍니다. [`Collection::cascade`](../../api/rust/collection.md#cascade)로 선언합니다.

### default

```rust
pub fn default(&self) -> Option<&Value>
```

객체에서 빠졌을 때 들어가는 [`Value`](./value.md)입니다. 기본값이 있는 필수 필드에만 있습니다.
