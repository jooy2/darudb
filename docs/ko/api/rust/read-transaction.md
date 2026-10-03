---
title: ReadTransaction
order: 3
---

# ReadTransaction

`ReadTransaction`은 커밋 하나를 기준으로 데이터베이스를 일관되게 보여 주는 읽기 트랜잭션입니다. 저장 커널의 트리와 스키마의 컬렉션을 읽습니다.

```rust
#[derive(Debug)]
pub struct ReadTransaction
```

[`Database::begin_read`](./database.md#begin-read)로 시작합니다. 살아 있는 동안 무엇이 커밋되든, 이 트랜잭션으로 읽는 것은 모두 시작할 때의 커밋에서 나옵니다. 버리면 끝나고, 커밋할 것은 없습니다. 그때까지 이 트랜잭션이 닿을 수 있는 페이지는 다시 쓰이지 않으므로, 읽기 트랜잭션을 오래 열어 두면 다른 쪽이 쓰는 동안 파일이 커집니다. `Send`이자 `Sync`입니다.

저장 커널은 이름 붙은 트리에 바이트 키와 바이트 값을 저장하고, 키는 부호 없는 바이트 순서로 정렬합니다. 트리는 [저장 커널](../../engine/storage-kernel.md)에서 설명합니다. 트리 이름은 1바이트 이상이고 페이지 크기가 허용하는 가장 긴 키보다 길 수 없습니다. 4096바이트 페이지에서는 957바이트입니다. 비었거나 그보다 긴 이름은 `INVALID_ARGUMENT`로 실패합니다. NUL 문자로 시작하는 이름은 컬렉션의 객체를 담는 엔진 전용 트리의 것이어서 같은 오류로 실패합니다. 없는 트리를 읽으면 아무것도 나오지 않습니다. 컬렉션은 [`collection`](#collection)으로 읽습니다.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;
    let read = db.begin_read()?;

    if let Some(role) = read.get("users", b"alice")? {
        println!("alice is {}", String::from_utf8_lossy(&role));
    }

    for entry in read.range("users", b"a".as_slice()..b"c".as_slice())? {
        let (key, value) = entry?;

        println!(
            "{} = {}",
            String::from_utf8_lossy(&key),
            String::from_utf8_lossy(&value)
        );
    }

    Ok(())
}
```

## 메서드

### commit_id

```rust
pub fn commit_id(&self) -> u64
```

이 트랜잭션이 보는 커밋의 트랜잭션 ID입니다. 커밋할 때마다 커지기만 하므로, 이전에 얻은 값과 비교하면 그 사이에 커밋이 있었는지 알 수 있습니다.

### get

```rust
pub fn get(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>>
```

`tree` 트리에서 `key`에 저장된 값을 돌려줍니다. 없으면 `None`입니다. 없는 트리에는 아무것도 없는 것으로 봅니다.

### iter

```rust
pub fn iter(&self, tree: &str) -> Result<Range<'_>>
```

`tree` 트리의 모든 항목을 키 순서대로, 키와 값의 [`Range`](../../types/rust/range.md)로 돌려줍니다.

### range

```rust
pub fn range<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

`tree` 트리에서 키가 `range` 안에 드는 항목을 키 순서대로 돌려줍니다. 바이트 문자열의 범위라면 무엇이든 됩니다. `b"a".as_slice()..b"c".as_slice()`는 `a`부터 `c` 직전까지의 키를, `key..`는 `key`부터 끝까지의 키를 돕니다.

### range_backward

```rust
pub fn range_backward<K: AsRef<[u8]>>(
    &self,
    tree: &str,
    range: impl RangeBounds<K>,
) -> Result<Range<'_>>
```

`tree` 트리에서 키가 `range` 안에 드는 항목을 마지막 키부터 거꾸로 돌려줍니다. 키가 커지는 트리라면 가장 최근 항목부터 읽게 됩니다.

### len

```rust
pub fn len(&self, tree: &str) -> Result<u64>
```

`tree` 트리의 항목 수입니다. 트리가 없으면 0입니다. 항목 수는 트리와 함께 기록돼 있어서 항목을 하나도 읽지 않습니다.

### tree_names

```rust
pub fn tree_names(&self) -> Result<Vec<String>>
```

모든 트리의 이름을 바이트 순서로 돌려줍니다. 컬렉션의 객체를 담는 엔진 전용 트리는 빠집니다.

### collection

```rust
pub fn collection(&self, name: &str) -> Result<CollectionReader<'_>>
```

핸들을 열 때 쓴 스키마에서 `name` 컬렉션을 꺼내 객체를 읽을 수 있게 합니다. [`CollectionReader`](./collection-reader.md)를 보세요. 트랜잭션을 빌려 쓰며, 여러 컬렉션을 한꺼번에 읽어도 됩니다.

스키마에 그런 컬렉션이 없거나 스키마 없이 연 데이터베이스면 `INVALID_ARGUMENT`로 실패합니다. 이 핸들을 연 뒤에 다른 핸들이나 프로세스가 파일을 마이그레이션해서, 이 트랜잭션이 보는 커밋에 다른 스키마가 있으면 `SCHEMA_MISMATCH`로 실패합니다.
