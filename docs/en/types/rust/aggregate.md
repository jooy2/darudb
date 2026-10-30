---
title: Aggregate
order: 12.5
group: queries
pageClass: reference-page
---

# Aggregate

`Aggregate` names what an aggregate works out from the values of one field of the objects a query finds.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Aggregate {
    Sum,
    Min,
    Max,
    Avg,
}
```

[`CollectionReader::aggregate`](../../api/rust/collection-reader.md#aggregate) takes one, for a program that chooses the function as it runs; [`sum`](../../api/rust/collection-reader.md#sum), [`min`](../../api/rust/collection-reader.md#min), [`max`](../../api/rust/collection-reader.md#max) and [`avg`](../../api/rust/collection-reader.md#avg) name theirs. Null values, and paths that reach no value, are left out.

| Variant | Reads | Gives |
| --- | --- | --- |
| `Sum` | `int`, `float` | The exact sum: an int for an int field, a float for a float field; zero for none |
| `Min` | `int`, `float`, `string`, `bytes`, `date` | The least value, in the order a sort gives, or null |
| `Max` | `int`, `float`, `string`, `bytes`, `date` | The greatest value, or null |
| `Avg` | `int`, `float` | The mean as a float, the exact sum over the count rounded once, or null |

## Methods

### code

```rust
pub fn code(self) -> i64
```

The function's code in the IR: 1 for `Sum`, 2 for `Min`, 3 for `Max` and 4 for `Avg`.

### from_code

```rust
pub fn from_code(code: i64) -> Option<Self>
```

The function whose code in the IR is `code`, if any is.
