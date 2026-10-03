---
title: Migrating
order: 13
---

# Migrating

`Migrating`은 마이그레이션 함수가 받는 마이그레이션의 쓰기 트랜잭션입니다. 컬렉션은 새 스키마의 것이고, 객체는 예전 스키마대로 읽을 수 있습니다.

```rust
#[derive(Debug)]
pub struct Migrating<'a>
```

[`Migration::run`](./migration.md#run)에 준 함수가 `&mut Migrating`을 받습니다. [`collection`](#collection)은 선언한 스키마의 컬렉션을 꺼내며, 여러 버전에 걸친 마이그레이션의 중간 단계에서도 마찬가지입니다. [`previous`](#previous)와 [`previous_keys`](#previous-keys)는 마이그레이션 전체를 시작하기 전에 파일에 있던 스키마의 이름으로 객체를 읽고, 마이그레이션이 지우거나 교체한 필드의 값도 읽습니다. 이렇게 읽을 수 있는 것은 마이그레이션이 커밋되기 전까지입니다. 엔진은 모든 함수가 반환한 뒤에 트랜잭션을 커밋합니다.

객체를 쓰면 새 스키마의 필드만 남으므로, 함수는 객체를 쓰기 전에 `previous`로 읽어 둡니다.

```rust
use darudb::{Collection, Migration, Object, OpenOptions, Schema, Type};

fn main() -> darudb::Result<()> {
    let v2 = Schema::new(2).collection(
        Collection::new("users")
            .field("name", Type::String)
            .with_default("age", Type::String, ""),
    );
    let migration = Migration::to(2)
        .replace_field("users", "age")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let age = migrating
                    .previous("users", key.clone())?
                    .and_then(|user| user.get("age")?.as_int())
                    .unwrap_or(0);
                let mut users = migrating.collection("users")?;

                users.update(key, Object::new().with("age", format!("{age} years")))?;
            }

            Ok(())
        });
    let db = OpenOptions::new()
        .schema(v2)
        .migration(migration)
        .open("app.darudb")?;

    db.close()
}
```

## 메서드

### previous_version

```rust
pub fn previous_version(&self) -> u64
```

마이그레이션 전에 파일에 있던 스키마 버전입니다. 여러 버전에 걸친 마이그레이션이라면 모든 단계에서 같은 값입니다.

### collection

```rust
pub fn collection(&mut self, name: &str) -> Result<CollectionWriter<'_>>
```

새 스키마에서 `name` 컬렉션을 꺼내 객체를 읽고 쓸 수 있게 합니다. [`CollectionWriter`](./collection-writer.md)를 보세요. 새 스키마에 없는 컬렉션이면 `INVALID_ARGUMENT`로 실패합니다.

### previous_keys

```rust
pub fn previous_keys(&self, collection: &str) -> Result<Vec<Value>>
```

`collection`에 있는 모든 객체의 기본 키를 키 순서대로 돌려줍니다. 컬렉션은 마이그레이션 전 스키마의 이름으로 씁니다. 그 스키마에 없는 컬렉션이면 `INVALID_ARGUMENT`로 실패합니다.

### previous

```rust
pub fn previous(&self, collection: &str, key: impl Into<Value>) -> Result<Option<Object>>
```

`collection`에서 기본 키가 `key`인 객체를 마이그레이션 전 스키마대로 읽습니다. 컬렉션과 필드는 그 스키마의 이름으로 나오고, 마이그레이션이 지우거나 교체한 필드의 값도 들어 있습니다. 마이그레이션이 지울 컬렉션도 커밋되기 전까지는 이렇게 읽을 수 있습니다.

객체는 지금 있는 그대로 읽습니다. 함수가 이미 쓴 객체라면, 마이그레이션이 없앤 필드는 그 필드가 없는 레코드를 읽을 때처럼 기본값이나 null로 나옵니다. 그러니 객체를 쓰기 전에 이렇게 읽어 두세요.

### transaction

```rust
pub fn transaction(&mut self) -> &mut WriteTransaction
```

마이그레이션이 도는 쓰기 트랜잭션입니다. 컬렉션 말고 저장 커널의 트리를 읽고 쓸 때 씁니다. 커밋은 모든 마이그레이션 함수가 반환한 뒤 엔진이 합니다.

### previous_record

```rust
pub fn previous_record(
    &self,
    collection: &str,
    key: impl Into<Value>,
) -> Result<Option<Vec<u8>>>
```

`previous`가 읽을 객체의 레코드를 파일에 있는 그대로 돌려줍니다. [`PendingMigration::previous_schema_record`](./opening.md#previous-schema-record)로 레코드를 직접 디코딩하는 언어 바인딩을 위한 메서드입니다. 레코드에 없는 필드는 필수든 아니든 기본값이나 null로 읽습니다.
