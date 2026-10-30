---
title: Index
order: 14.5
group: objects
counterpart: /types/node/stored-schema
pageClass: reference-page
---

# Index

`Index`는 컬렉션의 인덱스를 스키마가 선언한 모습 그대로 다시 읽은 것입니다. 인덱스를 둔 필드들의 순서와, 고유 인덱스인지를 담습니다.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index
```

필드 하나에 두는 인덱스는 [`Collection::index`](../../api/rust/collection.md#index)와 [`Collection::unique`](../../api/rust/collection.md#unique)로 선언하고, 여러 필드나 내장 객체 안의 필드에 두는 인덱스는 [`Collection::index_on`](../../api/rust/collection.md#index-on)과 [`Collection::unique_on`](../../api/rust/collection.md#unique-on)으로 선언합니다. [`Collection::indexes`](../../api/rust/collection.md#indexes)가 이를 다시 돌려주며, [`Database::stored_schema`](../../api/rust/database.md#stored-schema)가 파일에서 읽은 스키마에서도 마찬가지입니다.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;

    for collection in db.stored_schema().iter().flat_map(|schema| schema.collections()) {
        for index in collection.indexes() {
            let fields: Vec<String> = index.fields().collect();
            let unique = if index.is_unique() { " (unique)" } else { "" };

            println!("{}: {}{unique}", collection.name(), fields.join(", "));
        }
    }

    Ok(())
}
```

## 메서드

### fields

```rust
pub fn fields(&self) -> impl ExactSizeIterator<Item = String> + '_
```

인덱스를 둔 필드들을 인덱스 항목이 정렬되는 순서대로 돌려줍니다. 각 필드는 이름이거나, `address.city`처럼 내장 객체를 따라 이름을 `.`으로 이은 것입니다.

### is_unique

```rust
pub fn is_unique(&self) -> bool
```

인덱스의 모든 필드에서 같은 값을 가진 두 객체를 거부하는지 알려 줍니다. 어느 한 필드가 null인 객체는 몇 개가 있어도 됩니다.
