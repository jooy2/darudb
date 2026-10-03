---
title: Migration
order: 15
counterpart: /types/node/migration
---

# Migration

`Migration`은 스키마 버전 `n`이 버전 `n - 1`에서 무엇을 바꾸는지, 엔진이 알아서 하는 변경 말고 따로 적어야 하는 것을 담습니다. 이름 바꾸기, 필드 교체, 컬렉션 삭제, 데이터를 옮기는 함수가 여기에 들어갑니다.

```rust
#[derive(Clone)]
pub struct Migration
```

[`OpenOptions::migration`](./open-options.md#migration)으로 더합니다. 더 낮은 스키마 버전을 가진 파일을 열면, 파일 버전의 다음 버전부터 선언한 버전까지의 마이그레이션을 버전 순서대로 실행합니다. 엔진이 알아서 하는 변경과 함께 쓰기 트랜잭션 하나 안에서 실행하므로, 마이그레이션 전체가 커밋되거나, 파일이 예전 스키마와 데이터를 그대로 유지하고 `open`이 실패하거나 둘 중 하나입니다. `Migration`은 `Debug`, `Send`, `Sync`를 구현합니다.

새 컬렉션, 선택 필드나 기본값이 있는 새 필드, 필드 삭제, 인덱스 추가와 삭제는 엔진이 알아서 합니다. 그 밖에 바꾸는 것이 없는 버전에는 `Migration`이 필요 없습니다. 컬렉션이나 필드의 이름 바꾸기, 필드의 타입 바꾸기, 스키마에서 컬렉션 빼기에는 `Migration`이 필요합니다. `Migration` 없이 열면, 사라진 컬렉션이나 타입이 바뀐 필드는 `open`이 `INVALID_ARGUMENT`로 실패합니다. 이름만 바꾼 필드는 필드 하나를 지우고 새 필드를 더한 것으로 읽히므로, 예전 값은 새 필드로 옮겨지지 않습니다. 변경마다의 설명은 [마이그레이션](../../guide/migrations.md)에 있습니다.

```rust
use darudb::{Collection, Migration, OpenOptions, Schema, Type};

fn main() -> darudb::Result<()> {
    let v2 = Schema::new(2).collection(
        Collection::new("people")
            .field("full_name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::String, "")
            .unique("email"),
    );
    let migration = Migration::to(2)
        .rename_collection("users", "people")
        .rename_field("users", "name", "full_name")
        .replace_field("users", "age")
        .delete_collection("posts")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let before = migrating.previous("users", key.clone())?;
                let age = before
                    .and_then(|user| user.get("age")?.as_int())
                    .unwrap_or(0);
                let mut people = migrating.collection("people")?;

                if let Some(mut person) = people.get(key)? {
                    person.set("age", format!("{age} years"));
                    people.put(person)?;
                }
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

마이그레이션은 컬렉션과 필드를 이전 스키마의 이름으로 부릅니다. 변경은 준 순서와 상관없이 필드 이름 바꾸기, 필드 교체, 컬렉션 삭제, 컬렉션 이름 바꾸기 순으로 적용합니다. 그래서 같은 마이그레이션이 컬렉션 이름을 바꾸더라도 `rename_field`, `replace_field`, `delete_collection`에는 컬렉션의 예전 이름을 씁니다. 이전 스키마에 없는 컬렉션이나 필드를 쓰거나, 이미 있는 이름으로 바꾸려 하면 `open`이 `INVALID_ARGUMENT`로 실패합니다. 같은 버전으로 가는 마이그레이션이 둘이어도 마찬가지입니다.

## 연관 함수

### to

```rust
pub fn to(version: u64) -> Self
```

이전 버전에서 스키마 버전 `version`으로 가는 마이그레이션을 만듭니다. 버전은 2부터 선언한 스키마의 버전까지 쓸 수 있고, 그 밖의 값은 `open`이 `INVALID_ARGUMENT`로 실패합니다.

## 메서드

### rename_collection

```rust
pub fn rename_collection(mut self, from: impl Into<String>, to: impl Into<String>) -> Self
```

`from` 컬렉션의 이름을 `to`로 바꿉니다. 객체는 그 자리에 있으므로 객체가 아무리 많아도 비용이 없습니다.

### rename_field

```rust
pub fn rename_field(
    mut self,
    collection: impl Into<String>,
    from: impl Into<String>,
    to: impl Into<String>,
) -> Self
```

`collection`의 `from` 필드 이름을 `to`로 바꿉니다. 컬렉션은 이 마이그레이션이 이름을 바꾸기 전의 이름으로 씁니다. 객체를 다시 쓰지 않습니다.

### replace_field

```rust
pub fn replace_field(
    mut self,
    collection: impl Into<String>,
    field: impl Into<String>,
) -> Self
```

`collection`의 `field` 필드를 같은 이름의 새 필드로 바꿉니다. 필드의 타입을 바꿀 때 씁니다. 파일에 이미 있는 컬렉션의 새 필드가 모두 그렇듯, 아직 값을 가진 객체가 없으므로 새 필드는 선택 필드이거나 기본값이 있어야 합니다. 예전 값은 마이그레이션 함수에서 [`Migrating::previous`](./migrating.md#previous)로 읽을 수 있습니다. 기본 키는 교체할 수 없습니다.

### delete_collection

```rust
pub fn delete_collection(mut self, name: impl Into<String>) -> Self
```

`name` 컬렉션을 객체와 인덱스까지 함께 지웁니다. 지우는 일은 마이그레이션의 맨 끝에 하므로, 마이그레이션 함수는 그때까지 [`Migrating::previous`](./migrating.md#previous)로 그 객체를 읽을 수 있습니다.

### run

```rust
pub fn run(
    mut self,
    function: impl Fn(&mut Migrating<'_>) -> Result<()> + Send + Sync + 'static,
) -> Self
```

스키마가 새것으로 바뀐 뒤, 같은 쓰기 트랜잭션 안에서 `function`을 마이그레이션의 일부로 실행합니다. 함수는 [`Migrating`](./migrating.md)을 받아, 객체를 예전 스키마대로 읽고 새 스키마로 씁니다. 함수가 오류를 돌려주면 마이그레이션은 끝나고 파일은 예전 스키마와 데이터를 그대로 유지하며, `open`은 그 오류로 실패합니다. 애플리케이션이 실패 이유를 직접 적으려면 코드가 `MIGRATION_FAILED`인 `Error::MigrationFailed`를 돌려줍니다. [`Error`](../../types/rust/error.md)를 보세요.
