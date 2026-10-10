---
title: CompactReport
order: 10
group: tools
pageClass: reference-page
---

# CompactReport

`CompactReport` is what a compaction did: the size of the file before and after, and how many pages it moved.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompactReport
```

[`Database::compact`](../../api/rust/database.md#compact) returns one. [Tools](../../guide/tools.md) explains how compaction works while others read and write. The struct is `#[non_exhaustive]`, so fields may be added in a release: read the fields by name, and use `..` when destructuring one.

```rust
use darudb::Database;

fn compact(db: &Database) -> darudb::Result<()> {
    let report = db.compact()?;

    println!("{} bytes, then {}", report.bytes_before, report.bytes_after);
    Ok(())
}
```

## Fields

| Field          | Type  | Description                            |
| -------------- | ----- | -------------------------------------- |
| `bytes_before` | `u64` | The size of the file before, in bytes  |
| `bytes_after`  | `u64` | The size of the file after, in bytes   |
| `pages_moved`  | `u64` | The pages moved out of the file's tail |

A page that a read transaction can still reach does not move until the transaction ends, so `bytes_after` can be larger than the data needs while long readers are open. The next compaction takes the rest.
