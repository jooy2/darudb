---
title: ChangeWhere
order: 8.6
---

# ChangeWhere

`ChangeWhere`는 타입이 있는 컬렉션에서 쿼리가 찾은 모든 객체에 대한 변경을 필드 하나씩 모았다가, 전부 되거나 전혀 안 되게 씁니다.

```rust
pub struct ChangeWhere<'w, 'a, T>
```

[`TypedWriter::change_where`](./typed-writer.md#change-where)가 쿼리를 받아 만듭니다. [`Change`](./change.md)가 한 객체에서 하듯, 필드는 [`#[derive(Object)]`](./derive.md)가 선언한 상수인 [`FieldOf`](../../types/rust/field-of.md)로 지정하고 값은 그 필드의 타입으로 받습니다. [`apply`](#apply) 전에는 아무것도 쓰지 않습니다.

```rust
use darudb::{Database, Filter, Object, Query};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index, default = 0)]
    age: i64,
    active: bool,
}

fn deactivate_minors(db: &Database) -> darudb::Result<u64> {
    let mut txn = db.begin_write()?;
    let minors = Query::new().filter(Filter::lt("age", 18));
    let changed = txn
        .collection_of::<User>()?
        .change_where(&minors)
        .set(User::ACTIVE, false)
        .apply()?;

    txn.commit()?;
    Ok(changed)
}
```

## 메서드

### set

```rust
pub fn set<V: FieldType>(self, field: FieldOf<T, V>, value: V) -> Self
```

각 객체의 `field`를 `value`로 정합니다. 앞서 정한 값이 있으면 바꾸고, 변경을 돌려줍니다. [`Change::set`](./change.md#set)을 보세요.

### apply

```rust
pub fn apply(self) -> Result<u64>
```

쿼리가 정렬, 오프셋, 개수 제한을 거쳐 찾은 모든 객체에 변경을 쓰고, 그 개수를 돌려줍니다. [`CollectionWriter::update_where`](./collection-writer.md#update-where)처럼 전부 되거나 전혀 안 됩니다. 한 객체가 변경을 거부하면 앞서 바뀐 객체를 되돌리고, 트랜잭션은 호출 전 그대로 남습니다. 변경으로 기본 키를 설정할 수는 없습니다.
