---
title: QueryRequest
order: 18
---

# QueryRequest

`QueryRequest`는 언어의 경계를 넘나드는 형태의 쿼리입니다. 쿼리와, 쿼리를 실행할 컬렉션, 그리고 객체를 돌려주는 대신 셀지 집계를 구할지를 담습니다.

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct QueryRequest {
    pub collection: String,
    pub query: Query,
    pub count: bool,
    pub aggregate: Option<(Aggregate, Vec<String>)>,
}
```

애플리케이션에는 필요 없습니다. Rust 코드는 [`Query`](./query.md)를 만들어 컬렉션에서 실행합니다. 언어 바인딩은 자기 언어로 쿼리의 IR을 만들어 그 바이트를 한 번의 호출로 엔진에 넘깁니다. 바인딩의 네이티브 쪽은 [`decode`](#decode)로 이를 읽고, 트랜잭션에서 `collection`을 꺼낸 뒤, `count`와 `aggregate`에 따라 [`query_records`](./collection-reader.md#query-records), [`count`](./collection-reader.md#count), [`aggregate`](./collection-reader.md#aggregate) 중 하나로 `query`를 실행합니다. 경계는 [바인딩](../../engine/bindings.md)에서 설명하고, IR 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-ir)에 있습니다.

```rust
use darudb::{QueryRequest, ReadTransaction, Value};

/// What the query a binding encoded finds.
enum Found {
    Count(u64),
    Aggregate(Value),
    Records(Vec<Vec<u8>>),
}

fn run(txn: &ReadTransaction, ir: &[u8]) -> darudb::Result<Found> {
    let request = QueryRequest::decode(ir)?;
    let collection = txn.collection(&request.collection)?;

    if let Some((function, path)) = &request.aggregate {
        collection
            .aggregate(&request.query, *function, path)
            .map(Found::Aggregate)
    } else if request.count {
        collection.count(&request.query).map(Found::Count)
    } else {
        collection.query_records(&request.query).map(Found::Records)
    }
}
```

이 구조체는 `#[non_exhaustive]`이므로, 크레이트 밖의 코드는 구조체 리터럴 대신 [`new`](#new)로 만든 뒤 `count`나 `aggregate`를 설정합니다.

## 필드

| 필드         | 타입                               | 설명                                 |
| ------------ | ---------------------------------- | ------------------------------------ |
| `collection` | `String`                           | 쿼리를 실행할 컬렉션                 |
| `query`      | `Query`                            | 어떤 객체를 어떤 순서로 몇 개 찾을지 |
| `count`      | `bool`                             | 객체를 돌려주지 않고 셀지            |
| `aggregate`  | `Option<(Aggregate, Vec<String>)>` | 구할 집계와, 집계가 읽는 필드의 경로 |

## 연관 함수

### new

```rust
pub fn new(collection: impl Into<String>, query: Query) -> Self
```

`collection`에서 `query`가 찾는 객체를 돌려받는 요청을 만듭니다. 세지도 않고 집계도 구하지 않습니다.

### decode

```rust
pub fn decode(bytes: &[u8]) -> Result<Self>
```

`bytes`에 든 IR을 읽습니다. 디코딩되지 않거나, 모르는 연산자를 쓰거나, 연산자에 필요한 것이 빠진 IR은 `INVALID_QUERY`로 실패하며, 개수와 집계를 함께 요청하는 IR도 그렇습니다. IR에서 값이 없는 매개변수는 매개변수로 남으며, 나중에 [`Query::bind_encoded`](./query.md#bind-encoded)로 값을 채웁니다. 쿼리가 스키마와 맞는지는 실행할 때 검사합니다.

## 메서드

### encode

```rust
pub fn encode(&self) -> Result<Vec<u8>>
```

이 요청의 IR입니다. 매개변수를 묶은 쿼리의 IR에는 매개변수 값이 제자리에 들어가고, 묶지 않은 준비된 쿼리는 매개변수를 그대로 둡니다. 개수와 집계를 함께 요청하면 `INVALID_QUERY`로 실패합니다.
