---
title: CompactReport
order: 10
---

# CompactReport

`CompactReport`는 압축이 한 일로, 압축 전후의 파일 크기와 옮긴 페이지 수를 담습니다.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompactReport
```

[`Database::compact`](../../api/rust/database.md#compact)가 돌려줍니다. 다른 쪽이 읽고 쓰는 동안 압축이 어떻게 동작하는지는 [도구](../../guide/tools.md)에 있습니다. 이 구조체에는 `#[non_exhaustive]`가 붙어 있어 릴리스에서 필드가 늘 수 있습니다. 필드는 이름으로 읽고, 구조 분해할 때는 `..`을 붙입니다.

```rust
use darudb::Database;

fn compact(db: &Database) -> darudb::Result<()> {
    let report = db.compact()?;

    println!("{} bytes, then {}", report.bytes_before, report.bytes_after);
    Ok(())
}
```

## 필드

| 필드           | 타입  | 설명                         |
| -------------- | ----- | ---------------------------- |
| `bytes_before` | `u64` | 압축 전 파일 크기(바이트)    |
| `bytes_after`  | `u64` | 압축 후 파일 크기(바이트)    |
| `pages_moved`  | `u64` | 파일 끝쪽에서 옮긴 페이지 수 |

읽기 트랜잭션이 아직 닿을 수 있는 페이지는 그 트랜잭션이 끝날 때까지 옮기지 않습니다. 그래서 오래 도는 읽기가 있으면 `bytes_after`가 데이터에 필요한 크기보다 클 수 있습니다. 나머지는 다음 압축이 처리합니다.
