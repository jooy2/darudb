---
title: TypedReader
order: 7
---

# TypedReader

`TypedReader`는 읽기 트랜잭션이 보는 컬렉션 하나의 객체를 그 컬렉션의 Rust 타입으로 바로 읽습니다.

```rust
pub struct TypedReader<'a, T>
```

[`#[derive(Object)]`](./derive.md)가 구현하는 [`CollectionType`](../../types/rust/collection-type.md) 타입에 대해 [`ReadTransaction::collection_of`](./read-transaction.md#collection-of)가 돌려줍니다. [`CollectionReader`](./collection-reader.md)와 같은 일을 하되 [`Object`](../../types/rust/object.md) 대신 `T`를 돌려줍니다. 레코드를 그 자리에서 `T`로 풀기 때문에, `Object`를 만들 때 드는 이름 붙은 값의 벡터를 거치지 않습니다.

```rust
use darudb::{Database, Filter, Object, Query};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index)]
    age: i64,
}

fn read(db: &Database) -> darudb::Result<()> {
    let read = db.begin_read()?;
    let users = read.collection_of::<User>()?;

    if let Some(user) = users.get(1)? {
        println!("{}", user.name);
    }

    for user in users.query(&Query::new().filter(Filter::ge("age", 18)).sort_by("name"))? {
        println!("{} is {}", user.name, user.age);
    }

    Ok(())
}
```

## 메서드

### get

```rust
pub fn get(&self, key: impl Into<Value>) -> Result<Option<T>>
```

기본 키가 `key`인 객체를 돌려주고, 없으면 `None`을 돌려줍니다. 컬렉션의 키와 타입이 다른 키는 `INVALID_ARGUMENT`로 실패합니다.

### query

```rust
pub fn query(&self, query: &Query) -> Result<Vec<T>>
```

[`query`](./query.md)가 찾는 객체를 그 순서대로 돌려줍니다. 실패하는 경우는 [`CollectionReader::query`](./collection-reader.md#query)와 같습니다.

### count

```rust
pub fn count(&self, query: &Query) -> Result<u64>
```

`query`가 찾는 객체 수를 오프셋을 건너뛰고 한도 안에서 셉니다. [`CollectionReader::count`](./collection-reader.md#count)와 같은 방식으로 셉니다.

### iter

```rust
pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_>
```

모든 객체를 기본 키 순서로 돌려줍니다. 파일이 손상된 곳에서는 항목이 오류입니다.

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
pub fn untyped(&self) -> &CollectionReader<'a>
```

같은 컬렉션을 [`CollectionReader`](./collection-reader.md)로 돌려줍니다. 언어 바인딩이 쓰는 레코드처럼 타입 리더에 없는 기능이 필요할 때 씁니다.
