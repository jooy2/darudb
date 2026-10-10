---
title: BackupReport
order: 9
group: tools
pageClass: reference-page
---

# BackupReport

`BackupReport`는 백업이 쓴 내용으로, 복사한 커밋과 그 양, 새 파일의 크기를 담습니다.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupReport
```

[`Database::backup`](../../api/rust/database.md#backup)이 새 파일을 온전하게 디스크에 기록한 뒤 돌려줍니다. 다른 쪽이 쓰는 동안 백업이 어떻게 동작하는지는 [도구](../../guide/tools.md)에 있습니다. 이 구조체에는 `#[non_exhaustive]`가 붙어 있어 릴리스에서 필드가 늘 수 있습니다. 필드는 이름으로 읽고, 구조 분해할 때는 `..`을 붙입니다.

```rust
use darudb::Database;

fn back_up(db: &Database) -> darudb::Result<()> {
    let report = db.backup("backups/app.darudb")?;

    println!("{} entries of commit {}, {} bytes", report.entries, report.commit_id, report.bytes);
    Ok(())
}
```

## 필드

| 필드        | 타입  | 설명                                                               |
| ----------- | ----- | ------------------------------------------------------------------ |
| `commit_id` | `u64` | 복사한 커밋의 트랜잭션 id. 백업을 시작할 때 게시돼 있던 커밋입니다 |
| `trees`     | `u64` | 복사한 트리 수. 엔진 자체의 트리도 포함합니다                      |
| `entries`   | `u64` | 모든 트리에서 복사한 항목 수                                       |
| `bytes`     | `u64` | 새 파일의 크기(바이트)                                             |

`trees`와 `entries`는 애플리케이션의 트리뿐 아니라 엔진 자체의 트리도 셉니다. 스키마가 있는 데이터베이스에서는 컬렉션 하나가 객체를 담는 트리 하나와 인덱스마다 트리 하나로 저장되므로, 이 수는 컬렉션과 객체의 수보다 큽니다.
