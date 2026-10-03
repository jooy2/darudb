---
title: CollectionWriter
order: 6
counterpart: /api/node/write-collection
---

# CollectionWriter

`CollectionWriter`는 쓰기 트랜잭션 안에서 컬렉션 하나의 객체를 트랜잭션의 변경까지 반영해 읽고, 객체를 넣고 바꾸고 고치고 지웁니다.

```rust
#[derive(Debug)]
pub struct CollectionWriter<'a>
```

[`WriteTransaction::collection`](./write-transaction.md#collection)이 돌려줍니다. 트랜잭션을 가변으로 빌리므로 한 번에 컬렉션 하나만 쓸 수 있고, 트랜잭션을 커밋하기 전에 그 빌림이 끝나야 합니다. 트랜잭션에서 다른 컬렉션을 꺼내면 앞의 것은 더 쓸 수 없습니다. 읽는 메서드는 트랜잭션의 변경을 반영한다는 점 말고는 [`CollectionReader`](./collection-reader.md)와 같고, 키를 넘기는 방법도 같습니다.

거부된 쓰기는 아무것도 바꾸지 않으므로, 트랜잭션은 계속 쓰다가 커밋해도 됩니다.

- **`DUPLICATE_KEY`**: `insert`가 이미 있는 기본 키를 만났거나, 고유 인덱스에서 객체의 값을 다른 객체가 이미 가지고 있을 때.
- **`INVALID_ARGUMENT`**: 객체가 스키마에 맞지 않을 때. 필드가 아닌 이름, 타입이 틀린 값, 기본값 없이 빠진 필수 필드, 파일의 키에 담기에 너무 긴 키나 인덱스 값, 4 GiB 이상인 레코드가 여기에 해당합니다.

여러 메서드를 함께 쓰는 예는 [컬렉션과 객체](../../guide/objects.md)에 있습니다.

```rust
use darudb::{Database, Object, Value};

fn write(db: &Database) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;

    let alice = users.insert(
        Object::new()
            .with("name", "Alice")
            .with("email", "alice@example.com"),
    )?;

    users.update(
        alice.clone(),
        Object::new().with("age", 32).with("email", Value::Null),
    )?;
    users.put(
        Object::new()
            .with("id", alice)
            .with("name", "Alice")
            .with("age", 33),
    )?;
    users.delete(7)?;

    txn.commit()
}
```

## 메서드

### insert

```rust
pub fn insert(&mut self, object: Object) -> Result<Value>
```

`object`를 넣고 기본 키를 돌려줍니다. 자동 증가 키를 쓰는 컬렉션에서 `id`가 없거나 null인 객체는 1부터 차례로 다음 번호를 받습니다. 객체를 지워도 한 파일 안에서 같은 번호를 두 번 주지 않습니다. `id`를 직접 정한 객체는 그 번호를 그대로 쓰고, 그다음 번호는 그보다 큰 수에서 시작합니다.

### put

```rust
pub fn put(&mut self, object: Object) -> Result<Value>
```

`object`를 넣거나, 기본 키가 같은 객체가 있으면 바꾸고 키를 돌려줍니다. 실패하는 경우는 `insert`와 같지만, 키가 이미 있는 것은 실패가 아닙니다. 바꾸기가 거부되면 원래 객체는 그대로 남습니다.

### update

```rust
pub fn update(&mut self, key: impl Into<Value>, changes: Object) -> Result<bool>
```

기본 키가 `key`인 객체에서 `changes`에 있는 필드만 바꾸고, 객체가 있었는지 돌려줍니다. 객체가 없으면 아무것도 쓰지 않습니다. 바뀐 객체는 저장된 객체에 그 필드를 넣어 `put`한 것과 같습니다. `Value::Null`을 주면 필드는 null이 되고, 기본값이 있는 필수 필드면 기본값이 됩니다. 기본값이 없는 필수 필드는 null로 만들 수 없습니다. 내장 객체와 목록은 통째로 바뀝니다. 객체를 읽어 `put`하는 것보다 비용이 적습니다.

실패하는 경우는 `put`과 같고, `changes`에 `key`와 다른 기본 키가 있으면 `INVALID_ARGUMENT`로 실패합니다.

### delete

```rust
pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool>
```

기본 키가 `key`인 객체를 인덱스 항목과 함께 지우고, 객체가 있었는지 돌려줍니다.

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>>
```

기본 키가 `key`인 객체를 이 트랜잭션의 변경까지 반영해 돌려줍니다. 없으면 `None`입니다.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_>
```

모든 객체를 이 트랜잭션의 변경까지 반영해 기본 키 순서대로 돌려줍니다.

### len

```rust
pub fn len(&self) -> Result<u64>
```

이 트랜잭션의 변경까지 반영한 객체 수입니다.

### is_empty

```rust
pub fn is_empty(&self) -> Result<bool>
```

컬렉션에 객체가 하나도 없는지 알려 줍니다.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<Object>>
```

[`query`](./query.md)가 찾은 객체를 이 트랜잭션의 변경까지 반영해 그 쿼리의 순서대로 돌려줍니다. 실패하는 경우는 [`CollectionReader::query`](./collection-reader.md#query)와 같습니다.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

`query`가 찾는 객체 수를 이 트랜잭션의 변경까지 반영해 셉니다. [`CollectionReader::count`](./collection-reader.md#count)를 보세요.

### insert_record

```rust
pub fn insert_record(&mut self, record: &[u8]) -> Result<Value>
```

언어 바인딩이 보낸 레코드 `record`로 객체를 넣습니다. 레코드에는 객체가 가진 필드가 ID로 들어 있고, 쓰기는 `insert`와 똑같이 이를 검사하고 빠진 값을 채웁니다. 디코딩되지 않거나, 컬렉션에 없는 ID나 타입이 들어 있는 레코드는 `INVALID_ARGUMENT`로 실패합니다. 레코드 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#records)에 있습니다.

### put_record

```rust
pub fn put_record(&mut self, record: &[u8]) -> Result<Value>
```

레코드 `record`로 객체를 넣거나 바꿉니다. 객체를 `put`하는 것과 같습니다. 레코드는 `insert_record`와 같은 형식입니다.

### update_record

```rust
pub fn update_record(&mut self, key: impl Into<Value>, changes: &[u8]) -> Result<bool>
```

기본 키가 `key`인 객체에서, 언어 바인딩이 보낸 레코드 `changes`에 있는 필드만 바꿉니다. 필드는 ID로 들어 있고, null로 만들 필드에는 태그 `0x01`이 들어 있습니다. 나머지는 `update`와 같습니다. 필드가 모두 스칼라인 컬렉션에서는 저장된 레코드를 그 자리에서 고치고, 바뀐 필드의 인덱스만 읽습니다.

### get_record

```rust
pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>>
```

기본 키가 `key`인 객체의 레코드를 이 트랜잭션의 변경까지 반영해 돌려줍니다. 언어 바인딩을 위한 메서드이며, [`CollectionReader::get_record`](./collection-reader.md#get-record)를 보세요.

### get_record_with

```rust
pub fn get_record_with(
    &self,
    key: impl Into<Value>,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool>
```

기본 키가 `key`인 객체의 레코드를 이 트랜잭션의 변경까지 반영해, 복사하지 않고 빌려 `visit`에 넘깁니다. [`CollectionReader::get_record_with`](./collection-reader.md#get-record-with)를 보세요.

### query_records

```rust
pub fn query_records(&self, query: &Query) -> Result<Vec<Vec<u8>>>
```

`query`가 찾은 객체의 레코드를 이 트랜잭션의 변경까지 반영해 돌려줍니다. [`CollectionReader::query_records`](./collection-reader.md#query-records)를 보세요.

### query_records_with

```rust
pub fn query_records_with(
    &self,
    query: &Query,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()>
```

`query`가 찾은 객체의 레코드를 이 트랜잭션의 변경까지 반영해 하나씩 `visit`에 넘깁니다. [`CollectionReader::query_records_with`](./collection-reader.md#query-records-with)를 보세요.
