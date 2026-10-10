---
title: 저장 커널
order: 2
languages: [rust]
---

# 저장 커널

저장 커널은 컬렉션 아래에 있는 계층으로, 이름 붙은 트리에 바이트 키와 바이트 값을 담아 트랜잭션으로 읽고 쓰며, Rust 프로그램은 이 계층을 바로 쓸 수 있습니다.

## 바이트를 담는 트리

데이터베이스에는 트리를 몇 개든 둘 수 있습니다. 트리마다 이름이 있고, 키와 값으로 이뤄진 항목을 담습니다. 키와 값은 모두 바이트이며 커널은 거기에 아무 뜻도 붙이지 않습니다. 컬렉션과 인덱스를 포함해 엔진이 저장하는 모든 것이 트리에 들어가고, 트랜잭션과 내구성, 암호화, 여러 프로세스의 공유는 모두 커널이 제공합니다.

- **이름**은 문자열이며, 길이는 1바이트부터 키의 최대 길이까지입니다. NUL 문자로 시작하는 이름은 엔진이 컬렉션과 인덱스를 두는 자리입니다. 모든 호출이 그런 이름을 `INVALID_ARGUMENT`로 거부하고, `tree_names`에도 나오지 않습니다.
- **키는 부호 없는 바이트 순서로 정렬됩니다.** 어떤 키가 다른 키의 앞부분이면 짧은 쪽이 먼저 옵니다. 커널은 이 밖의 순서를 모르므로, 원하는 순서대로 바이트가 정렬되게 키를 인코딩해야 합니다. 예를 들어 부호 없는 정수는 빅엔디언으로 쓰면 값 순서대로 정렬됩니다.
- **키**는 최대 ⌊(페이지 크기 − 268) / 4⌋바이트입니다. 기본 페이지 크기 4096에서는 957바이트, 16384에서는 4029바이트, 65536에서는 16317바이트입니다. 이 제한이 있어서 노드 하나에 항목 네 개가 늘 들어갑니다. 빈 키도 다른 키와 똑같이 쓸 수 있습니다.
- **값**은 4 GiB 미만입니다. 키와 합쳐 페이지의 4분의 1쯤을 넘어 트리 페이지에 담기 어려운 값은 따로 페이지를 차지하고, 읽을 때 통째로 읽습니다.
- **트리**는 처음 넣을 때 생기고, 없는 트리는 비어 있는 것으로 읽힙니다. 항목을 모두 지우면 빈 트리로 남아 목록에 계속 나오며, 트리 자체는 `delete_tree`로 지웁니다.

## 쓰기 트랜잭션

모든 변경은 쓰기 트랜잭션 안에서 일어나고, 트랜잭션이 커밋될 때 다른 변경과 함께 한꺼번에 보입니다.

```rust
use darudb::Database;

fn main() -> Result<(), darudb::Error> {
    let db = Database::open("app.darudb")?;

    let mut txn = db.begin_write()?;
    txn.insert("users", b"alice", b"admin")?;
    txn.insert("users", b"bob", b"member")?;
    txn.commit()?;

    let read = db.begin_read()?;
    assert_eq!(read.get("users", b"alice")?, Some(b"admin".to_vec()));

    // Keys come back in byte order.
    for entry in read.range("users", b"a".as_slice()..b"c".as_slice())? {
        let (key, value) = entry?;
        println!("{} = {}", String::from_utf8_lossy(&key), String::from_utf8_lossy(&value));
    }

    Ok(())
}
```

- `insert`는 키에 값을 저장하고, 이미 값이 있으면 바꿉니다. `remove`는 키를 지우고 키가 있었는지 돌려주며, `delete_tree`는 트리를 통째로 지웁니다.
- 트랜잭션 안에서 읽는 `get`, `range`, `range_backward`, `iter`, `len`, `tree_names`는 그 트랜잭션의 변경을 반영합니다.
- 트랜잭션을 버리거나 `abort`를 부르면 변경은 사라지고, 파일의 커밋된 상태에는 아무것도 남지 않습니다.
- 쓰기 트랜잭션은 파일마다, 모든 프로세스를 통틀어 한 번에 하나입니다. `begin_write`는 이 프로세스나 다른 프로세스에서 이미 실행 중인 트랜잭션을 바쁨 대기 시간만큼 기다리고, 그래도 끝나지 않으면 `BUSY`로 실패합니다. 바쁨 대기 시간은 `OpenOptions::busy_timeout`으로 바꾸지 않으면 5초입니다. 쓰기 트랜잭션을 쥔 스레드가 쓰기 트랜잭션을 또 시작하면 자기 자신을 기다리다가 똑같이 실패합니다.
- `insert`나 `remove`, `delete_tree`가 한 번이라도 실패하면, 예를 들어 키가 제한보다 길면, 그 트랜잭션은 취소할 수밖에 없습니다. `commit`이 `INVALID_ARGUMENT`로 실패합니다. NUL로 시작하는 이름만은 트랜잭션을 건드리기 전에 거부되므로 예외입니다. 컬렉션은 다르게 동작해서, 객체 쓰기가 거부돼도 트랜잭션은 그대로 커밋할 수 있습니다.

## 읽기 트랜잭션

읽기 트랜잭션은 살아 있는 동안 커밋 하나, 곧 자기 스냅샷만 봅니다. 시작한 뒤에 이 프로세스나 다른 프로세스가 무엇을 커밋하든 마찬가지입니다. 읽기 트랜잭션은 쓰는 쪽을 기다리지 않고, 쓰는 쪽도 읽기 트랜잭션을 기다리지 않습니다.

- `get`, `range`, `range_backward`, `iter`, `len`은 트리 하나를 읽고, `tree_names`는 트리 목록을 돌려줍니다.
- `commit_id`는 트랜잭션이 보는 커밋의 트랜잭션 ID입니다. 트랜잭션 ID는 늘어나기만 하므로, 두 값을 비교하면 그사이에 커밋이 있었는지 알 수 있습니다.
- 스냅샷이 닿는 페이지는 트랜잭션을 버릴 때까지 어느 프로세스에서도 다시 쓰이지 않습니다. 읽기 트랜잭션을 오래 열어 두면 다른 쪽이 쓰는 동안 파일이 커지므로, 필요한 것을 읽었으면 바로 버리세요.

## 범위 읽기

`range`는 바이트 문자열의 범위를 받아, 키가 그 범위에 드는 항목을 키 순서대로 돌려줍니다. `range_backward`는 같은 항목을 마지막 것부터 거꾸로 돌려주고, `iter`는 모든 항목을 돌려줍니다. 항목마다 키와 값이 나오며, 순회에 필요한 페이지가 손상됐으면 그 자리에서 오류가 나오고 순회가 끝납니다. 범위는 트랜잭션을 빌려 씁니다.

빅엔디언 숫자처럼 키가 커지는 트리는 가장 최근 항목이 끝에 있고, `range_backward`는 거기서 시작합니다.

```rust
use darudb::Database;

fn append(db: &Database, sequence: u64, line: &str) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;

    // Big-endian, so that the keys' byte order is their numeric order.
    txn.insert("log", &sequence.to_be_bytes(), line.as_bytes())?;
    txn.commit_deferred()
}

fn newest(db: &Database, count: usize) -> Result<Vec<String>, darudb::Error> {
    let read = db.begin_read()?;
    let mut lines = Vec::new();

    for entry in read.range_backward::<&[u8]>("log", ..)?.take(count) {
        let (_, value) = entry?;

        lines.push(String::from_utf8_lossy(&value).into_owned());
    }

    Ok(lines)
}
```

같은 접두사로 시작하는 키를 모두 읽으려면 범위를 접두사에서 시작하고, 접두사로 시작하지 않는 키가 처음 나올 때 멈춥니다.

```rust
use darudb::ReadTransaction;

fn with_prefix(read: &ReadTransaction, prefix: &[u8]) -> Result<Vec<Vec<u8>>, darudb::Error> {
    let mut keys = Vec::new();

    for entry in read.range("users", prefix..)? {
        let (key, _) = entry?;

        if !key.starts_with(prefix) {
            break;
        }

        keys.push(key);
    }

    Ok(keys)
}
```

## 내구성

`commit`은 변경이 내구성을 가진 뒤에 반환합니다. 프로세스가 비정상 종료되거나 정전이 나도 변경은 남습니다. `commit_deferred`는 디스크를 기다리지 않고 반환합니다. 변경은 곧바로 읽기 트랜잭션에 보이고, 다음 동기화 때 내구성을 갖습니다. 다음 `commit`이나 `Database::sync`, 데이터베이스를 닫을 때, 또는 기다릴 수 있는 양의 제한에 닿을 때가 그렇습니다. 제한은 기본값으로 1초와 16,384페이지이며 `OpenOptions::max_unsynced_time`과 `OpenOptions::max_unsynced_pages`로 바꿉니다. 프로세스가 비정상 종료돼도 지연 커밋은 하나도 잃지 않습니다. 정전이 나면 가장 최근 것부터 되돌려질 수 있지만 중간이 빠지지는 않습니다. 두 커밋 방식은 [커밋과 복구](./commits-and-recovery.md)에서 설명합니다.

## 한 파일 안의 트리와 컬렉션

스키마를 주고 연 데이터베이스에서도 커널은 그대로 쓸 수 있습니다. 트리와 컬렉션이 한 파일에 함께 있고, 쓰기 트랜잭션 하나로 둘을 함께 바꿔 한꺼번에 커밋할 수 있습니다.

```rust
use darudb::{Database, Object};

fn add_user(db: &Database, png: &[u8]) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;

    txn.collection("users")?
        .insert(Object::new().with("name", "Alice"))?;
    txn.insert("thumbnails", b"alice", png)?;
    txn.commit()
}
```

`tree_names`에는 커널의 트리만 나옵니다. 컬렉션은 엔진 전용 트리에 들어 있기 때문입니다.

## 커널을 쓸 때

처음에는 컬렉션으로 시작하세요. 커널은 이런 데이터에 어울립니다.

- **이미 바이트인 데이터**, 또는 직접 정한 형식으로 인코딩한 데이터. 파일, 캐시한 응답, 직렬화한 구조체가 그렇습니다.
- **순서를 직접 정하고 싶은 데이터.** 빅엔디언 타임스탬프 뒤에 ID를 붙인 키처럼, 범위 하나로 원하는 것을 찾도록 키를 짤 수 있습니다.
- **컬렉션이 더해 주는 기능을 쓰지 않을 만큼 단순한 데이터.**

컬렉션에는 커널에 없는 것이 있습니다. 스키마와 대조하는 타입 있는 필드, 객체와 맞춰 두는 인덱스, 인덱스를 골라 쓰는 쿼리, 스키마 버전 사이의 마이그레이션, 모든 객체를 인덱스와 대조하는 무결성 검사입니다. 무엇보다 커널은 Rust에서만 쓸 수 있습니다. Node.js와 Dart, Python 패키지는 컬렉션만 읽고 쓰므로, 다른 언어가 읽어야 하는 데이터는 컬렉션에 두어야 합니다. 다른 언어의 프로그램은 커널의 트리를 건드리지 않고, 백업과 압축과 되살리기도 이 트리를 그대로 옮깁니다.

## 호출

| 호출 | 하는 일 |
| --- | --- |
| `Database::begin_write` | 쓰기 트랜잭션을 시작합니다. 이미 실행 중인 트랜잭션은 바쁨 대기 시간만큼 기다립니다. |
| `Database::begin_read` | 마지막 커밋을 보는 읽기 트랜잭션을 시작합니다. |
| `Database::sync` | 어느 프로세스가 했든 지연 커밋을 모두 동기화합니다. |
| `insert`, `remove`, `delete_tree` | [쓰기 트랜잭션](../api/rust/write-transaction.md)에서 트리를 바꿉니다. |
| `get`, `range`, `range_backward`, `iter`, `len`, `tree_names` | [읽기 트랜잭션](../api/rust/read-transaction.md)이나 쓰기 트랜잭션에서 읽습니다. 범위는 [`Range`](../types/rust/range.md)로 돌려줍니다. |
| `commit`, `commit_deferred`, `abort` | 쓰기 트랜잭션을 끝냅니다. |
| `commit_id` | 읽기 트랜잭션이 보는 커밋의 트랜잭션 ID입니다. |

모든 트리에 적용되는 옵션, 곧 바쁨 대기 시간과 페이지 크기, 캐시 크기, 지연 커밋의 제한은 [`Database`](../api/rust/database.md)와 [`OpenOptions`](../api/rust/open-options.md)에 있습니다.
