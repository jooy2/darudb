---
title: QueryRequest
order: 15
---

# QueryRequest

`QueryRequest`는 언어의 경계를 넘나드는 형태의 쿼리입니다. 쿼리와, 쿼리를 실행할 컬렉션, 객체를 돌려줄지 셀지를 담습니다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRequest {
    pub collection: String,
    pub query: Query,
    pub count: bool,
}
```

애플리케이션에는 필요 없습니다. Rust 코드는 [`Query`](./query.md)를 만들어 컬렉션에서 실행합니다. 언어 바인딩은 자기 언어로 쿼리의 IR을 만들어 그 바이트를 한 번의 호출로 엔진에 넘깁니다. 바인딩의 네이티브 쪽은 [`decode`](#decode)로 이를 읽고, 트랜잭션에서 `collection`을 꺼낸 뒤, `count`에 따라 [`query_records`](./collection-reader.md#query-records)나 [`count`](./collection-reader.md#count)로 `query`를 실행합니다. 경계는 [바인딩](../../engine/bindings.md)에서 설명하고, IR 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-ir)에 있습니다.

```rust
use darudb::{QueryRequest, ReadTransaction};

/// What the query a binding encoded finds.
enum Found {
    Count(u64),
    Records(Vec<Vec<u8>>),
}

fn run(txn: &ReadTransaction, ir: &[u8]) -> darudb::Result<Found> {
    let request = QueryRequest::decode(ir)?;
    let collection = txn.collection(&request.collection)?;

    if request.count {
        collection.count(&request.query).map(Found::Count)
    } else {
        collection.query_records(&request.query).map(Found::Records)
    }
}
```

## 필드

| 필드         | 타입     | 설명                                 |
| ------------ | -------- | ------------------------------------ |
| `collection` | `String` | 쿼리를 실행할 컬렉션                 |
| `query`      | `Query`  | 어떤 객체를 어떤 순서로 몇 개 찾을지 |
| `count`      | `bool`   | 객체를 돌려주지 않고 셀지            |

## 연관 함수

### decode

```rust
pub fn decode(bytes: &[u8]) -> Result<Self>
```

`bytes`에 든 IR을 읽습니다. 디코딩되지 않거나, 모르는 연산자를 쓰거나, 연산자에 필요한 것이 빠진 IR은 `INVALID_QUERY`로 실패합니다. IR에서 값이 없는 매개변수는 매개변수로 남으며, 나중에 [`Query::bind_encoded`](./query.md#bind-encoded)로 값을 채웁니다. 쿼리가 스키마와 맞는지는 실행할 때 검사합니다.

## 메서드

### encode

```rust
pub fn encode(&self) -> Result<Vec<u8>>
```

이 요청의 IR을 돌려줍니다. 값을 채운 쿼리의 IR에는 매개변수 자리에 값이 들어가고, 값을 채우지 않은 준비한 쿼리는 매개변수를 그대로 둡니다.
