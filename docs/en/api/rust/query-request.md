---
title: QueryRequest
order: 18
---

# QueryRequest

`QueryRequest` is a query as it crosses a language boundary: the query, the collection it runs on, and whether it counts the objects or works out an aggregate of them rather than returning them.

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

An application does not need it: Rust code builds a [`Query`](./query.md) and runs it on a collection. A language binding builds the query's IR in its own language and hands the engine the bytes in one call. The binding's native side reads them with [`decode`](#decode), opens `collection` in its transaction, and runs `query` there with [`query_records`](./collection-reader.md#query-records), [`count`](./collection-reader.md#count) or [`aggregate`](./collection-reader.md#aggregate), as `count` and `aggregate` say. [Bindings](../../engine/bindings.md) describes the boundary, and [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-ir) specifies the IR.

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

The struct is `#[non_exhaustive]`, so code outside the crate makes one with [`new`](#new) and sets `count` or `aggregate` on it, rather than with a struct literal.

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `collection` | `String` | The collection the query runs on |
| `query` | `Query` | What to find, in what order, and how many |
| `count` | `bool` | Whether to count the objects rather than return them |
| `aggregate` | `Option<(Aggregate, Vec<String>)>` | The aggregate to work out, and the path of the field it reads |

## Associated functions

### new

```rust
pub fn new(collection: impl Into<String>, query: Query) -> Self
```

A request for the objects `query` finds in `collection`, neither counting them nor working out an aggregate.

### decode

```rust
pub fn decode(bytes: &[u8]) -> Result<Self>
```

Reads the IR in `bytes`. IR that does not decode, or that names an unknown operator or leaves out what one uses, fails with `INVALID_QUERY`, and so does IR that both counts and works out an aggregate. Parameters the IR leaves without values stay parameters, and the query is bound later with [`Query::bind_encoded`](./query.md#bind-encoded). The query is checked against the schema only when it runs.

## Methods

### encode

```rust
pub fn encode(&self) -> Result<Vec<u8>>
```

The IR of this request. A bound query's IR has its parameters' values in their places, and a prepared query that is not bound keeps its parameters. A request that both counts and works out an aggregate fails with `INVALID_QUERY`.
