---
title: WriteTransaction
order: 4
---

# WriteTransaction

`WriteTransaction`은 데이터베이스의 변경을 모았다가 커밋할 때 한꺼번에 보이게 하는 쓰기 트랜잭션입니다. 커밋하지 않으면 아무것도 남지 않습니다.

```rust
#[derive(Debug)]
pub struct WriteTransaction
```

[`Database::begin_write`](./database.md#begin-write)로 시작합니다. 쓰기 트랜잭션은 모든 스레드와 프로세스를 통틀어 파일마다 하나뿐이고, 읽기 트랜잭션은 이를 기다리지 않습니다. 트랜잭션 안에서 읽으면 자기가 바꾼 내용이 보입니다. [`commit`](#commit)은 변경이 디스크에 기록된 뒤에 반환하고, [`commit_deferred`](#commit-deferred)는 디스크를 기다리지 않고 변경을 게시합니다. 둘의 차이는 [트랜잭션](../../guide/transactions.md)에 있습니다. 커밋하지 않고 버리면 트랜잭션은 취소되고, 그 안에서 한 일은 파일에 남지 않습니다. `Send`이자 `Sync`이므로 다른 스레드로 넘길 수 있습니다.

트리를 바꾸다 실패하면, 예를 들어 너무 긴 키를 쓰면 그 트랜잭션은 더는 커밋할 수 없습니다. `commit`과 `commit_deferred`는 `INVALID_ARGUMENT`로 실패하고, 버리는 것만 남습니다. 컬렉션에 쓰다가 거부된 경우는 예외입니다. 거부된 쓰기는 아무것도 바꾸지 않으므로 트랜잭션은 계속 쓰다가 커밋해도 됩니다. 어떤 쓰기가 거부되는지는 [`CollectionWriter`](./collection-writer.md)에 있습니다.

트리와 트리 이름의 규칙은 [`ReadTransaction`](./read-transaction.md)과 같습니다. 키는 페이지의 4분의 1에서 몇 바이트를 뺀 길이까지 쓸 수 있고, 기본 페이지 크기에서는 957바이트입니다. 값은 4 GiB보다 짧아야 하며, 트리의 페이지에 담기에 너무 큰 값은 따로 페이지를 잡아 저장합니다.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;
    let mut txn = db.begin_write()?;

    txn.insert("users", b"alice", b"admin")?;
    txn.insert("users", b"bob", b"member")?;
    txn.remove("users", b"carol")?;
    txn.commit()?;

    db.close()
}
```

## 메서드

### insert

```rust
pub fn insert(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()>
```

`tree` 트리의 `key`에 `value`를 저장합니다. 이미 값이 있으면 바꿉니다. 트리가 없으면 새로 만듭니다.

### remove

```rust
pub fn remove(&mut self, tree: &str, key: &[u8]) -> Result<bool>
```

`tree` 트리에서 `key`와 그 값을 지우고, 키가 있었는지 돌려줍니다.

### delete_tree

```rust
pub fn delete_tree(&mut self, tree: &str) -> Result<bool>
```

`tree` 트리를 안에 든 것과 함께 지우고, 트리가 있었는지 돌려줍니다.

### get

```rust
pub fn get(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>>
```

`tree` 트리에서 `key`에 저장된 값을 이 트랜잭션의 변경까지 반영해 돌려줍니다.

### iter

```rust
pub fn iter(&self, tree: &str) -> Result<Range<'_>>
```

`tree` 트리의 모든 항목을 이 트랜잭션의 변경까지 반영해 키 순서대로, [`Range`](../../types/rust/range.md)로 돌려줍니다. 도는 동안에는 트랜잭션을 빌려 쓰므로 아무것도 바꿀 수 없습니다.

### range

```rust
pub fn range<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

`tree` 트리에서 키가 `range` 안에 드는 항목을 이 트랜잭션의 변경까지 반영해 키 순서대로 돌려줍니다. 범위는 [`ReadTransaction::range`](./read-transaction.md#range)와 같은 방법으로 줍니다.

### range_backward

```rust
pub fn range_backward<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

`tree` 트리에서 키가 `range` 안에 드는 항목을 이 트랜잭션의 변경까지 반영해 마지막 키부터 거꾸로 돌려줍니다.

### len

```rust
pub fn len(&self, tree: &str) -> Result<u64>
```

`tree` 트리의 항목 수를 이 트랜잭션의 변경까지 반영해 돌려줍니다. 트리가 없으면 0입니다.

### tree_names

```rust
pub fn tree_names(&self) -> Result<Vec<String>>
```

모든 트리의 이름을 바이트 순서로 돌려줍니다. 이 트랜잭션에서 만들거나 지운 트리도 반영합니다. 컬렉션의 객체를 담는 엔진 전용 트리는 빠집니다.

### collection

```rust
pub fn collection(&mut self, name: &str) -> Result<CollectionWriter<'_>>
```

핸들을 열 때 쓴 스키마에서 `name` 컬렉션을 꺼내 객체를 읽고 쓸 수 있게 합니다. [`CollectionWriter`](./collection-writer.md)를 보세요. 트랜잭션을 가변으로 빌리므로 한 번에 컬렉션 하나만 쓸 수 있고, 트랜잭션을 커밋하기 전에 그 빌림이 끝나야 합니다. 실패하는 경우는 [`ReadTransaction::collection`](./read-transaction.md#collection)과 같습니다.

### commit

```rust
pub fn commit(mut self) -> Result<()>
```

이 트랜잭션의 모든 변경을 한꺼번에 보이게 하고 디스크에 기록합니다. 반환한 뒤에는 크래시나 정전이 나도 변경이 남습니다. `SYNC_FAILED`로 실패하면 결과를 알 수 없으므로 데이터베이스를 다시 열어야 합니다. 앞서 변경 하나가 실패했다면 `INVALID_ARGUMENT`로 실패합니다.

### commit_deferred

```rust
pub fn commit_deferred(mut self) -> Result<()>
```

이 트랜잭션의 모든 변경을 한꺼번에 보이게 하되, 디스크에 기록될 때까지 기다리지 않습니다. 반환하자마자 읽기 트랜잭션에 변경이 보입니다. 변경은 다음 동기화 때 디스크에 기록됩니다. 다음 `commit`, [`Database::sync`](./database.md#sync) 호출, 데이터베이스 닫기, 또는 [`OpenOptions::max_unsynced_pages`](./open-options.md#max-unsynced-pages)와 [`max_unsynced_time`](./open-options.md#max-unsynced-time)의 한도가 그때입니다. 한도를 넘게 될 지연 커밋은 그 커밋에서 바로 디스크에 기록됩니다.

프로세스가 비정상 종료돼도 지연 커밋은 하나도 잃지 않습니다. 정전이 나면 가장 최근 것부터 되돌려질 수 있지만, 중간이 빠지거나 파일이 손상되지는 않습니다. 실패하는 경우는 `commit`과 같습니다.

### abort

```rust
pub fn abort(self)
```

이 트랜잭션의 모든 변경을 버립니다. 트랜잭션을 그냥 버리는 것과 같습니다.
