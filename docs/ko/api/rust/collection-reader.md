---
title: CollectionReader
order: 5
counterpart: /api/node/read-collection
---

# CollectionReader

`CollectionReader`는 읽기 트랜잭션이 보는 컬렉션 하나의 객체를 기본 키로, 키 순서대로 모두, 또는 쿼리로 찾아 읽습니다.

```rust
#[derive(Debug)]
pub struct CollectionReader<'a>
```

[`ReadTransaction::collection`](./read-transaction.md#collection)이 돌려줍니다. 트랜잭션을 빌려 쓰므로 트랜잭션보다 오래 살 수 없지만, 돌려주는 객체는 둘보다 오래 남는 평범한 [`Object`](../../types/rust/object.md) 값입니다. 읽어 온 객체에는 스키마의 필드가 모두 들어 있습니다. 객체를 쓸 때 빠진 필드에는 기본값이나 null이 들어갑니다.

기본 키는 키 타입의 [`Value`](../../types/rust/value.md)로 바뀌는 값이면 무엇이든 넘길 수 있습니다. 자동 증가 `id`라면 정수를, 문자열 키라면 `&str`을 넘기는 식입니다. 타입이 다른 키는 `INVALID_ARGUMENT`로 실패합니다.

```rust
use darudb::{Database, Filter, Query};

fn read(db: &Database) -> darudb::Result<()> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    if let Some(user) = users.get(1)? {
        println!("{:?}", user.get("name"));
    }

    let adults = Query::new().filter(Filter::ge("age", 18)).sort_by("name");

    for user in users.query(&adults)? {
        println!("{:?}", user.get("name"));
    }

    println!(
        "{} of {} users are adults",
        users.count(&adults)?,
        users.len()?
    );
    Ok(())
}
```

## 메서드

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>>
```

기본 키가 `key`인 객체를 돌려줍니다. 없으면 `None`입니다.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_>
```

모든 객체를 기본 키 순서대로 돌려줍니다. 파일이 손상된 곳에서는 항목이 오류로 나옵니다.

### len

```rust
pub fn len(&self) -> Result<u64>
```

객체 수입니다. 개수는 컬렉션과 함께 기록돼 있어서 객체를 하나도 읽지 않습니다.

### is_empty

```rust
pub fn is_empty(&self) -> Result<bool>
```

컬렉션에 객체가 하나도 없는지 알려 줍니다.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<Object>>
```

[`query`](./query.md)가 찾은 객체를 그 쿼리의 순서대로 돌려줍니다. 컬렉션에 없는 필드를 쓰거나, 필드를 타입이 다른 값과 비교하거나, 준비한 쿼리를 매개변수 값 없이 실행하면 `INVALID_QUERY`로 실패합니다. 어떤 쿼리가 인덱스를 읽고 어떤 쿼리가 모든 객체를 읽는지는 [쿼리](../../guide/queries.md)에서 설명합니다.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

`query`가 찾는 객체 수를 오프셋을 건너뛰고 개수 제한 안에서 셉니다. 인덱스나 기본 키만으로 답할 수 있는 필터라면 객체를 읽지 않고 셉니다. 실패하는 경우는 `query`와 같습니다.

### get_record

```rust
pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>>
```

기본 키가 `key`인 객체의 레코드를 파일에 있는 그대로 돌려줍니다. 레코드를 직접 디코딩하는 언어 바인딩을 위한 메서드이며, 인코딩 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#records)에 있습니다. 여기서는 검사하지 않으므로 바인딩은 이 레코드를 믿지 말고 다뤄야 합니다. 필드가 생기기 전에 쓴 레코드에는 그 필드가 없으며, 이때는 기본값이나 null로 읽습니다. 스키마에서 사라진 필드의 ID가 남아 있을 수도 있는데, 이런 필드는 건너뜁니다.

### get_record_with

```rust
pub fn get_record_with(
    &self,
    key: impl Into<Value>,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool>
```

`get_record`가 돌려줄 레코드를 따로 벡터에 복사하지 않고, 있는 자리에서 빌려 `visit`에 넘깁니다. 레코드가 있었는지 돌려줍니다. 레코드를 자기 버퍼로 복사하는 바인딩은 한 번만 복사하게 됩니다. `visit`이 돌려준 오류는 그대로 돌려줍니다.

### query_records

```rust
pub fn query_records(&self, query: &Query) -> Result<Vec<Vec<u8>>>
```

`query`가 찾은 객체의 레코드를 쿼리의 순서대로, 파일에 있는 그대로 돌려줍니다. 레코드를 직접 디코딩하는 언어 바인딩을 위한 메서드입니다. 필터와 정렬이 읽을 필요가 없는 객체는 아예 디코딩하지 않습니다.

### query_records_with

```rust
pub fn query_records_with(
    &self,
    query: &Query,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()>
```

`query`가 찾은 객체의 레코드를 쿼리의 순서대로 하나씩, 따로 벡터에 복사하지 않고 빌려 `visit`에 넘깁니다. `visit`이 처음 오류를 돌려주면 거기서 멈추고 그 오류를 돌려줍니다.
