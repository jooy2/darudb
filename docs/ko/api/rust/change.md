---
title: Change
order: 8.5
---

# Change

`Change`는 타입 있는 컬렉션의 객체 하나에 대한 변경을 필드 하나씩 모았다가 업데이트 한 번으로 씁니다.

```rust
pub struct Change<'w, 'a, T>
```

[`TypedWriter::change`](./typed-writer.md#change)가 받은 기본 키의 객체에 대해 만듭니다. 필드는 [`#[derive(Object)]`](./derive.md)가 필드마다 선언하는 상수인 [`FieldOf`](../../types/rust/field-of.md)로 가리키고, 값은 그 필드의 타입 그대로 넘깁니다. [`apply`](#apply)를 부르기 전에는 아무것도 쓰지 않습니다.

```rust
use darudb::{Database, Object};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
}

fn rename(db: &Database, id: i64) -> darudb::Result<bool> {
    let mut txn = db.begin_write()?;
    let updated = txn
        .collection_of::<User>()?
        .change(id)
        .set(User::NAME, "Alice Liddell".to_owned())
        .set(User::EMAIL, None)
        .set(User::AGE, 32)
        .apply()?;

    txn.commit()?;
    Ok(updated)
}
```

## 메서드

### set

```rust
pub fn set<V: FieldType>(self, field: FieldOf<T, V>, value: V) -> Self
```

`field`를 `value`로 바꾸도록 담고, 같은 필드에 먼저 담은 값이 있으면 대신합니다. 선택 필드에 `None`을 넘기면 null이 됩니다. 쓸 수 없는 값을 만나면 그 오류를 [`apply`](#apply)가 돌려주도록 남겨 둡니다.

### apply

```rust
pub fn apply(self) -> Result<bool>
```

담은 변경을 객체에 쓰고, 그 객체가 있었는지를 돌려줍니다. 없으면 아무것도 쓰지 않습니다. 실패하는 경우는 [`TypedWriter::set`](./typed-writer.md#set)과 같으며, 이때 트랜잭션은 그대로입니다.
