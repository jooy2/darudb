---
title: QueryRequest
order: 15
---

# QueryRequest

`QueryRequest` is a query as it crosses a language boundary: the query, the collection it runs on, and whether it counts the objects rather than returning them.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRequest {
    pub collection: String,
    pub query: Query,
    pub count: bool,
}
```

An application does not need it: Rust code builds a [`Query`](./query.md) and runs it on a collection. A language binding builds the query's IR in its own language and hands the engine the bytes in one call. The binding's native side reads them with [`decode`](#decode), opens `collection` in its transaction, and runs `query` there with [`query_records`](./collection-reader.md#query-records) or [`count`](./collection-reader.md#count), as `count` says. [Bindings](../../engine/bindings.md) describes the boundary, and [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-ir) specifies the IR.

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

## Fields

| Field        | Type     | Description                                          |
| ------------ | -------- | ---------------------------------------------------- |
| `collection` | `String` | The collection the query runs on                     |
| `query`      | `Query`  | What to find, in what order, and how many            |
| `count`      | `bool`   | Whether to count the objects rather than return them |

## Associated functions

### decode

```rust
pub fn decode(bytes: &[u8]) -> Result<Self>
```

Reads the IR in `bytes`. IR that does not decode, or that names an unknown operator or leaves out what one uses, fails with `INVALID_QUERY`. Parameters the IR leaves without values stay parameters, and the query is bound later with [`Query::bind_encoded`](./query.md#bind-encoded). The query is checked against the schema only when it runs.

## Methods

### encode

```rust
pub fn encode(&self) -> Result<Vec<u8>>
```

The IR of this request. A bound query's IR has its parameters' values in their places, and a prepared query that is not bound keeps its parameters.
