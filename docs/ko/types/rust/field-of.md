---
title: FieldOf
order: 6.5
group: objects
pageClass: reference-page
---

# FieldOf

`FieldOf<T, V>`는 컬렉션 타입 `T`에서 값의 타입이 `V`인 필드를 가리킵니다. 업데이트가 필드를 이름이 아니라 타입으로 가리키게 해 줍니다.

```rust
pub struct FieldOf<T, V>
```

[`#[derive(Object)]`](../../api/rust/derive.md#object)는 구조체의 필드마다 하나씩, 필드 이름을 대문자로 쓴 연관 상수를 필드와 같은 공개 범위로 선언합니다. `age` 필드라면 `User::AGE`이고 타입은 `FieldOf<User, i64>`입니다. [`TypedWriter::set`](../../api/rust/typed-writer.md#set)과 [`Change::set`](../../api/rust/change.md#set)은 이 상수와 `V` 타입의 값을 함께 받으므로, 다른 타입의 값을 넘기면 컴파일되지 않습니다.

```rust
use darudb::{Database, Object};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index, default = 0)]
    age: i64,
}

fn birthday(db: &Database, id: i64, age: i64) -> darudb::Result<bool> {
    let mut txn = db.begin_write()?;
    let updated = txn.collection_of::<User>()?.set(id, User::AGE, age + 1)?;

    txn.commit()?;
    Ok(updated)
}
```

상수의 값은 [`CollectionType::write_field`](./collection-type.md)가 세는 구조체 필드의 번호입니다. 컬렉션이 자동 증가 키를 쓰면 `id`가 0번입니다. `CollectionType`을 직접 구현한다면 `FieldOf::new`로 상수를 만듭니다.

## 메서드

### new

```rust
pub const fn new(slot: usize) -> Self
```

`T`에서 `slot`번인 필드입니다.

### slot

```rust
pub const fn slot(self) -> usize
```

이 필드의 `T` 안에서의 번호입니다.
