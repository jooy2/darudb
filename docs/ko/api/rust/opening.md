---
title: Opening과 PendingMigration
order: 14
---

# Opening과 PendingMigration

`Opening`과 `PendingMigration`을 쓰면 언어 바인딩이 데이터베이스를 열 때 마이그레이션을 버전 단계마다 하나씩 실행하면서, 단계마다 자기 함수도 함께 실행할 수 있습니다.

애플리케이션에는 필요 없습니다. 애플리케이션은 [`OpenOptions::open`](./open-options.md#open)으로 열고 마이그레이션 함수는 [`Migration::run`](./migration.md#run)에 줍니다. 바인딩은 마이그레이션 함수를 Rust 클로저로 엔진에 넘길 수 없으므로 `open_migrating`으로 열고, 마이그레이션은 버전 단계 사이마다 멈춰 바인딩에 차례를 넘깁니다. 바인딩이 이를 어떻게 쓰는지는 [바인딩](../../engine/bindings.md)에 있습니다.

```rust
use darudb::{Database, Migrating, OpenOptions, Opening};

/// Opens with `options`, running `step`, the binding's own function, in
/// every version step of a migration.
fn open(
    options: &OpenOptions,
    step: impl Fn(u64, &mut Migrating<'_>) -> darudb::Result<()>,
) -> darudb::Result<Database> {
    let mut pending = match options.open_migrating("app.darudb")? {
        Opening::Open(db) => return Ok(db),
        Opening::Migrating(pending) => pending,
    };

    while let Some(version) = pending.next_step()? {
        step(version, &mut pending.migrating())?;
    }

    pending.finish()
}
```

## open_migrating

```rust
pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening>
```

[`OpenOptions`](./open-options.md)의 메서드입니다. `open`처럼 데이터베이스를 열고 실패하는 경우도 같습니다. 다만 파일에 더 낮은 버전의 스키마가 있으면 마이그레이션을 끝까지 실행하지 않고, 진행 중인 마이그레이션을 담아 `Opening::Migrating`을 돌려줍니다.

## Opening

```rust
#[derive(Debug)]
pub enum Opening {
    Open(Database),
    Migrating(PendingMigration),
}
```

`open_migrating`으로 데이터베이스를 연 결과입니다.

| 배리언트    | 설명                                                             |
| ----------- | ---------------------------------------------------------------- |
| `Open`      | 데이터베이스가 열렸고, 선언한 스키마를 담고 있습니다             |
| `Migrating` | 파일에 더 낮은 버전의 스키마가 있어 마이그레이션이 진행 중입니다 |

### complete

```rust
pub fn complete(self) -> Result<Database>
```

남은 마이그레이션 단계를 모두 실행하고 커밋한 뒤 데이터베이스를 돌려줍니다. `Open`이면 그 데이터베이스를, `Migrating`이면 [`finish`](#finish)의 결과를 돌려줍니다.

## PendingMigration

```rust
#[derive(Debug)]
pub struct PendingMigration
```

진행 중인 마이그레이션입니다. 파일을 마이그레이션하는 쓰기 트랜잭션과, 아직 실행할 버전 단계를 담고 있습니다. 저장된 스키마는 이미 선언한 스키마로 바뀌었고 새 인덱스도 만들어져 있습니다. [`next_step`](#next-step)은 단계를 버전 순서대로 실행합니다. 단계마다 그 단계의 [`Migration`](./migration.md)이 등록한 함수가 있으면 실행하고 버전을 돌려주므로, 호출한 쪽은 [`migrating`](#migrating)으로 그 단계에 자기 함수를 실행할 수 있습니다. [`finish`](#finish)가 마이그레이션을 커밋하며, 그 대신 버리면 파일은 원래대로 남습니다.

그때까지 쓰기 트랜잭션을 쥐고 있으므로, 이 프로세스와 다른 프로세스의 쓰기는 이를 기다리다가 바쁨 대기 시간이 지나면 `BUSY`로 실패합니다.

### previous_version

```rust
pub fn previous_version(&self) -> u64
```

파일에 있는 스키마 버전입니다.

### version

```rust
pub fn version(&self) -> u64
```

마이그레이션이 끝나면 될 스키마 버전입니다.

### schema_record

```rust
pub fn schema_record(&self) -> &[u8]
```

마이그레이션이 끝나면 될 스키마를 파일이 저장하는 형식으로 인코딩해 돌려줍니다. 마이그레이션의 트랜잭션에는 이미 이 스키마가 들어 있습니다. [`Database::schema_record`](./database.md#schema-record)를 보세요.

### previous_schema_record

```rust
pub fn previous_schema_record(&self) -> Vec<u8>
```

마이그레이션 전에 파일에 있던 스키마를 파일이 저장하는 형식으로 인코딩해 돌려줍니다. [`Migrating::previous_record`](./migrating.md#previous-record)의 레코드를 직접 디코딩하는 바인딩이 씁니다.

### next_step

```rust
pub fn next_step(&mut self) -> Result<Option<u64>>
```

다음 버전 단계에 그 마이그레이션이 등록한 함수가 있으면 실행하고, 단계의 버전을 돌려줍니다. 모든 단계를 실행했으면 `None`입니다. 오류가 나면 마이그레이션은 끝납니다. 그때는 버리면 되고, 파일은 예전 스키마와 데이터를 그대로 유지합니다.

### migrating

```rust
pub fn migrating(&mut self) -> Migrating<'_>
```

마이그레이션 함수가 받는 형태로 마이그레이션의 쓰기 트랜잭션을 돌려줍니다. [`Migrating`](./migrating.md)을 보세요.

### transaction

```rust
pub fn transaction(&mut self) -> &mut WriteTransaction
```

마이그레이션의 쓰기 트랜잭션 자체를 돌려줍니다. 컬렉션은 새 스키마의 것입니다. 커밋은 `finish`가 합니다.

### finish

```rust
pub fn finish(self) -> Result<Database>
```

남은 단계를 실행하고, 단계들이 지우는 컬렉션을 지운 뒤 마이그레이션을 커밋합니다. 열린 데이터베이스를 돌려줍니다.
