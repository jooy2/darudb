---
title: TypedWriter
order: 8
---

# TypedWriter

`TypedWriter`는 쓰기 트랜잭션에서 컬렉션 하나의 객체를 그 컬렉션의 Rust 타입으로 읽고 씁니다.

```rust
pub struct TypedWriter<'a, T>
```

[`#[derive(Object)]`](./derive.md)가 구현하는 [`CollectionType`](../../types/rust/collection-type.md) 타입에 대해 [`WriteTransaction::collection_of`](./write-transaction.md#collection-of)가 돌려줍니다. [`CollectionWriter`](./collection-writer.md)와 같은 일을 하되 [`Object`](../../types/rust/object.md) 대신 `T`를 쓰고, 트랜잭션이 바꾼 내용도 봅니다. 트랜잭션을 가변으로 빌리므로 트랜잭션을 커밋하기 전에 그 빌림이 끝나야 합니다.

```rust
use darudb::{Database, Object};

#[derive(Object, Debug, Clone)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(default = 0)]
    age: i64,
}

fn write(db: &Database) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection_of::<User>()?;

    let id = users.insert(&User { id: None, name: "Alice".to_owned(), age: 31 })?;

    if let Some(mut alice) = users.get(id)? {
        alice.age += 1;
        users.put(&alice)?;
    }

    drop(users);
    txn.commit()
}
```

## 메서드

### insert

```rust
pub fn insert(&mut self, object: &T) -> Result<T::Key>
```

`object`를 넣고 기본 키를 돌려줍니다. 자동 증가 키를 쓰는 컬렉션에서는 `id`가 `None`인 객체가 다음 번호를 받고, `id`가 있는 객체는 그 번호를 그대로 씁니다. 키가 이미 있거나 고유 인덱스에 같은 값이 이미 있으면 `DUPLICATE_KEY`로 실패하고, 트랜잭션은 그 전 상태로 남습니다.

### put

```rust
pub fn put(&mut self, object: &T) -> Result<T::Key>
```

`object`를 넣거나, 같은 기본 키를 가진 객체를 바꾸고, 키를 돌려줍니다. 키가 이미 있는 것은 실패가 아니라는 점만 빼면 `insert`와 같은 경우에 실패합니다.

### delete

```rust
pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool>
```

기본 키가 `key`인 객체를 지우고, 지운 객체가 있었는지 돌려줍니다.

### update

```rust
pub fn update(&mut self, key: impl Into<Value>, changes: Object) -> Result<bool>
```

기본 키가 `key`인 객체에서 `changes`에 있는 필드만 바꾸고 나머지는 그대로 둡니다. [`CollectionWriter::update`](./collection-writer.md#update)와 같습니다. 구조체로 넘기면 모든 필드를 채워야 하므로, 바꿀 필드는 이름으로 담은 [`Object`](../../types/rust/object.md)로 넘깁니다. [`set`](#set)과 [`change`](#change)는 필드를 이름 대신 `T`로 가리킵니다.

### set

```rust
pub fn set<V: FieldType>(&mut self, key: impl Into<Value>, field: FieldOf<T, V>, value: V) -> Result<bool>
```

기본 키가 `key`인 객체의 `field`를 `value`로 바꾸고, 그 객체가 있었는지를 돌려줍니다. 없으면 아무것도 쓰지 않습니다. `field`는 [`#[derive(Object)]`](./derive.md#object)가 필드마다 선언하는 상수인 [`FieldOf`](../../types/rust/field-of.md)이고(`User::AGE` 같은), `value`는 그 필드의 타입 그대로입니다. 선택 필드에 `None`을 넘기면 null이 됩니다.

```rust
users.set(id, User::AGE, 32)?;
```

값은 타입 있는 쓰기와 같은 방식으로 씁니다. 필드가 모두 스칼라인 컬렉션이라면 저장된 레코드를 그 자리에서 고치므로, 객체를 `T`로 읽어 들이지도 통째로 다시 쓰지도 않습니다. 그래서 객체를 읽어 [`put`](#put)으로 다시 쓰는 것보다 빠릅니다. 실패하는 경우는 `update`와 같습니다. 고유 인덱스에 이미 있는 값이면 `DUPLICATE_KEY`, 기본 키를 `key`와 다른 값으로 바꾸려 하면 `INVALID_ARGUMENT`이고, 이때 트랜잭션은 그대로입니다.

### change

```rust
pub fn change(&mut self, key: impl Into<Value>) -> Change<'_, 'a, T>
```

기본 키가 `key`인 객체에 대한 변경을 돌려줍니다. [`Change::set`](./change.md#set)으로 필드를 하나씩 담고, [`Change::apply`](./change.md#apply)로 한 번의 업데이트로 씁니다.

```rust
users.change(id).set(User::NAME, "Alice Liddell".to_owned()).set(User::AGE, 32).apply()?;
```

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<T>>
```

이 트랜잭션이 바꾼 내용을 반영해, 기본 키가 `key`인 객체를 돌려주고 없으면 `None`을 돌려줍니다.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<T>>
```

이 트랜잭션이 바꾼 내용을 반영해, `query`가 찾는 객체를 그 순서대로 돌려줍니다.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

이 트랜잭션이 바꾼 내용을 반영해, `query`가 찾는 객체 수를 셉니다.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_>
```

이 트랜잭션이 바꾼 내용을 반영해, 모든 객체를 기본 키 순서로 돌려줍니다.

### len

```rust
pub fn len(&self) -> Result<u64>
```

객체 수입니다.

### is_empty

```rust
pub fn is_empty(&self) -> Result<bool>
```

컬렉션에 객체가 하나도 없는지 알려 줍니다.

### untyped

```rust
pub fn untyped(&mut self) -> &mut CollectionWriter<'a>
```

같은 컬렉션을 [`CollectionWriter`](./collection-writer.md)로 돌려줍니다. 타입 라이터에 없는 기능이 필요할 때 씁니다.
