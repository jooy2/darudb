---
title: CheckReport
order: 8
---

# CheckReport

`CheckReport`는 무결성 검사의 결과로, 검사한 커밋과 읽은 양, 찾은 문제를 모두 담습니다.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckReport
```

[`Database::check`](../../api/rust/database.md#check)가 돌려줍니다. 검사는 시작조차 할 수 없을 때만 실패하므로 손상은 오류가 아니라 이 보고서에 나타나며, [`is_ok`](#is-ok)가 참이면 파일이 검사를 통과했습니다. 검사가 무엇을 읽는지는 [도구](../../guide/tools.md)에 있습니다.

이 구조체에는 `#[non_exhaustive]`가 붙어 있어 릴리스에서 필드가 늘 수 있습니다. 필드는 이름으로 읽고, 구조 분해할 때는 `..`을 붙입니다.

```rust
use darudb::Database;

fn check(db: &Database) -> darudb::Result<()> {
    let report = db.check()?;

    if !report.is_ok() {
        for problem in &report.problems {
            eprintln!("{problem}");
        }
    }

    Ok(())
}
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `commit_id` | `u64` | 검사한 커밋의 트랜잭션 id. 검사를 시작할 때 게시돼 있던 커밋입니다 |
| `page_count` | `u64` | 그 커밋이 세는 페이지 수. 헤더 페이지도 포함합니다 |
| `pages_checked` | `u64` | 읽고 확인한 페이지 수 |
| `objects_checked` | `u64` | 읽어서 인덱스와 대조한 객체 수 |
| `problems` | `Vec<Problem>` | 찾은 문제 전부. 찾은 순서대로이며, 파일이 온전하면 비어 있습니다 |

`commit_id`는 같은 커밋에 대해 [`ReadTransaction::commit_id`](../../api/rust/read-transaction.md)가 돌려주는 값과 같습니다.

## 메서드

### is_ok

```rust
pub fn is_ok(&self) -> bool
```

검사에서 아무 문제도 찾지 못했는지, 곧 `problems`가 비어 있는지 알려 줍니다.

## Problem

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Problem
```

검사가 찾은 문제 하나입니다. 이 구조체에도 `#[non_exhaustive]`가 붙어 있습니다.

| 필드      | 타입             | 설명                                          |
| --------- | ---------------- | --------------------------------------------- |
| `page`    | `Option<u64>`    | 문제가 한 페이지 안에 있을 때 그 페이지       |
| `tree`    | `Option<String>` | 문제를 한 트리에서 찾았을 때 그 트리나 컬렉션 |
| `message` | `String`         | 무엇이 잘못됐는지                             |

`tree`는 사람이 읽으라고 있는 값입니다. 트리 이름이나 컬렉션 이름이 들어가지만, 엔진 자체의 트리에는 `the free tree` 같은 설명이 들어갈 수 있습니다. 그러니 프로그램이 이 값을 이름으로 찾아 쓰면 안 됩니다.

`Problem`은 `Display`를 구현합니다. 페이지와 트리를 알면 그것부터 쓰고 이어서 메시지를 씁니다. `page 12 of "users": ...` 같은 식입니다.
