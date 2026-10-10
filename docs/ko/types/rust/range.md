---
title: Range
order: 12
group: queries
pageClass: reference-page
---

# Range

`Range`는 트랜잭션의 `range`, `range_backward`, `iter`가 돌려주는 반복자로, 트리의 항목을 차례로 내놓습니다.

```rust
#[derive(Debug)]
pub struct Range<'a>
```

[`ReadTransaction`](../../api/rust/read-transaction.md)과 [`WriteTransaction`](../../api/rust/write-transaction.md)이 모두 이 반복자를 돌려줍니다. 저장소 커널이 바이트 키와 바이트 값을 담아 두는 이름 붙은 트리 하나를 훑으며, `range`와 `iter`는 키 순서로, `range_backward`는 마지막 키부터 거꾸로 내놓습니다. 키는 부호 없는 바이트로 비교합니다. 트리에 대해서는 [저장소 커널](../../engine/storage-kernel.md)에 있습니다.

반복자는 트랜잭션을 빌리므로 트랜잭션보다 오래 살 수 없습니다. 읽기 트랜잭션의 반복자는 그사이 무엇이 커밋되든 트랜잭션의 스냅샷을 보고, 쓰기 트랜잭션의 반복자는 그 트랜잭션이 지금까지 바꾼 내용을 봅니다. 반복자가 살아 있는 동안에는 빌림 때문에 쓰기 트랜잭션에서 아무것도 바꿀 수 없습니다.

## 항목

```rust
impl Iterator for Range<'_> {
    type Item = Result<(Vec<u8>, Vec<u8>)>;
}
```

항목 하나는 키와 그 값이며, 둘 다 페이지에서 복사해 옵니다. 없는 트리는 빈 트리처럼 훑습니다.

범위는 바이트 문자열로 쓴 Rust 범위라면 무엇이든 됩니다. `b"a".as_slice()..b"c".as_slice()`는 `a`부터 `c` 직전까지의 키를, `key..`는 `key`부터 끝까지의 키를 훑습니다. 경계가 없는 범위는 `range::<&[u8]>("users", ..)`처럼 키 타입을 적어 주거나, 대신 `iter`를 씁니다.

```rust
use darudb::Database;

fn newest(db: &Database, count: usize) -> darudb::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let read = db.begin_read()?;

    read.range_backward::<&[u8]>("events", ..)?.take(count).collect()
}
```

## 오류

반복자를 돌려주는 호출은 트리 이름이 비었거나, 페이지 크기가 허용하는 가장 긴 키보다 길거나, NUL 문자로 시작하면 `INVALID_ARGUMENT`로 실패합니다. NUL 문자로 시작하는 이름은 엔진 자체의 트리만 씁니다. 쓰기 트랜잭션의 호출은 그 파일에서 동기화가 실패한 뒤라면 `SYNC_FAILED`로 실패합니다.

호출은 훑기를 시작할 곳까지 페이지를 읽고, 항목을 하나 꺼낼 때마다 더 읽습니다. 그래서 둘 다 페이지가 부모가 기록한 검사값과 맞지 않으면 `CORRUPTED`로, 읽기에 실패하면 `IO`로 실패할 수 있습니다. 오류인 항목이 나오면 훑기가 끝나서 다음 `next`는 `None`을 돌려줍니다. 손상된 페이지는 건너뛸 수 없기 때문입니다.
