---
title: OpenOptions
order: 2
counterpart: /types/node/open-options
---

# OpenOptions

`OpenOptions`는 데이터베이스를 열 때 쓰는 옵션입니다. 파일을 새로 만들지, 페이지 크기는 얼마인지, 얼마나 기다리고 메모리를 얼마나 쓸지, 키나 비밀번호와 스키마는 무엇인지 정합니다.

```rust
#[derive(Debug, Clone)]
pub struct OpenOptions
```

`std::fs::OpenOptions`와 같은 방식의 빌더입니다. 옵션 메서드는 모두 `&mut self`를 받아 `&mut Self`를 돌려주므로, `OpenOptions::new()`에서 시작해 메서드를 이어 부르고 [`open`](#open)으로 끝냅니다. 이어 부른 결과는 임시 값의 참조이므로, 옵션을 두고 다시 쓰려면 먼저 변수에 담아야 합니다. `OpenOptions`는 `Default`도 구현하며, 이는 `new`와 같습니다.

```rust
use std::time::Duration;

use darudb::OpenOptions;

fn main() -> darudb::Result<()> {
    let db = OpenOptions::new()
        .create(false)
        .busy_timeout(Duration::from_secs(10))
        .open("app.darudb")?;

    db.close()?;

    let mut options = OpenOptions::new();

    options.password("correct horse battery staple");

    let secret = options.open("secret.darudb")?;

    secret.close()
}
```

한 프로세스에서 같은 파일을 연 핸들은 모두 인스턴스 하나를 공유하고, 몇몇 옵션은 이 인스턴스에 속합니다. 바쁨 대기 시간, 캐시 크기, 지연 커밋의 두 한도, 비밀번호 해시 비용은 파일을 처음 연 핸들의 값을 그 뒤에 연 핸들도 모두 따릅니다. 페이지 크기는 파일을 만들 때만 적용됩니다. 암호화한 파일은 핸들마다 키나 비밀번호를 줘야 하고, 스키마는 핸들마다 따로 가집니다.

## 기본값

| 옵션                 | 기본값                                   |
| -------------------- | ---------------------------------------- |
| `create`             | `true`                                   |
| `page_size`          | 4096바이트                               |
| `busy_timeout`       | 5초                                      |
| `cache_size`         | 32 MiB                                   |
| `max_unsynced_pages` | 16384페이지                              |
| `max_unsynced_time`  | 1초                                      |
| `password_hashing`   | 19456 KiB, 반복 2회, 레인 1개            |
| `key`, `password`    | 없음. 데이터베이스를 암호화하지 않습니다 |
| `schema`             | 없음. 데이터베이스에 컬렉션이 없습니다   |

## 연관 함수

### new

```rust
pub fn new() -> Self
```

위의 기본값으로 옵션을 만듭니다.

## 메서드

### create

```rust
pub fn create(&mut self, create: bool) -> &mut Self
```

경로에 아무것도 없을 때 데이터베이스를 새로 만들지 정합니다. 끄면 아무것도 없는 경로를 열 때 `NOT_FOUND`로 실패합니다. 어느 쪽이든 이미 있는 파일을 덮어쓰지는 않습니다.

### page_size

```rust
pub fn page_size(&mut self, bytes: u32) -> &mut Self
```

새 데이터베이스의 페이지 크기를 바이트 단위로 정합니다. 4096부터 65536까지의 2의 거듭제곱이어야 합니다. 이미 있는 파일은 헤더에 기록된 페이지 크기를 그대로 씁니다. 그 밖의 값을 주면, 파일이 이미 있더라도 `open`이 `INVALID_ARGUMENT`로 실패합니다.

### busy_timeout

```rust
pub fn busy_timeout(&mut self, timeout: Duration) -> &mut Self
```

[`Database::begin_write`](./database.md#begin-write)가 이 프로세스나 다른 프로세스에서 이미 실행 중인 쓰기 트랜잭션을 얼마나 기다릴지, 그리고 다른 프로세스가 파일을 복구하는 동안 여는 작업이 얼마나 기다릴지 정합니다. 이 시간이 지나면 `BUSY`로 실패합니다. `sync`, `close`, `compact`처럼 쓰기 트랜잭션을 기다리는 다른 메서드도 이 값을 씁니다.

### cache_size

```rust
pub fn cache_size(&mut self, bytes: usize) -> &mut Self
```

파일의 페이지 캐시가 쓸 수 있는 메모리를 바이트 단위로 정합니다. 캐시는 파일에서 읽어 검사하고 복호화한 페이지를 담아 두므로, 같은 페이지를 다시 읽을 때는 읽기도 검사도 하지 않습니다. `bytes`에 들어가는 만큼의 페이지를 담되 적어도 16개는 담습니다. 페이지를 읽는 만큼만 차므로 캐시보다 작은 데이터베이스는 그만큼을 다 쓰지 않습니다. 캐시에 다 들어가지 않는 데이터베이스는 캐시를 키우면 빨리 읽히고, 모바일 앱 확장처럼 메모리가 적은 프로세스라면 줄여도 됩니다.

### max_unsynced_pages

```rust
pub fn max_unsynced_pages(&mut self, pages: u64) -> &mut Self
```

지연 커밋이 몇 페이지까지 쓰면 그중 하나를 디스크에 기록할지 정합니다. 같은 페이지는 몇 번을 써도 한 번으로 셉니다. 16384페이지는 4096바이트 페이지로 64 MiB입니다. 이 한도가 있어서, 지연 커밋을 디스크에 기록하는 동기화가 쓸 양과 정전 뒤 복구가 검사할 양에 상한이 생깁니다. 한도를 넘게 될 지연 커밋은 그 커밋에서 바로 디스크에 기록됩니다.

### max_unsynced_time

```rust
pub fn max_unsynced_time(&mut self, time: Duration) -> &mut Self
```

지연 커밋이 동기화 없이 기다릴 수 있는 시간을 정합니다. 시간이 다 되면 엔진이 이를 위해 띄운 스레드가 [`Database::sync`](./database.md#sync)처럼 지연 커밋을 디스크에 기록합니다. 그때 쓰기 트랜잭션이 실행 중이면 스레드는 그 트랜잭션을 기다리고, 시간이 지난 뒤에 한 지연 커밋은 그 커밋에서 바로 디스크에 기록됩니다. 이 스레드는 기다리는 지연 커밋이 있는 동안에만 있습니다.

### key

```rust
pub fn key(&mut self, key: [u8; 32]) -> &mut Self
```

새 데이터베이스를 `key`로 암호화하거나, 암호화한 데이터베이스를 `key`로 엽니다. 암호화한 데이터베이스는 모든 페이지를 무작위 데이터 키로 암호화하고 인증하며, `key`는 이 데이터 키를 감쌉니다. 데이터베이스를 만드는 기기에 AES 명령어가 있으면 XAES-256-GCM을, 없으면 XChaCha20-Poly1305를 씁니다.

- **열 때의 오류.** 암호화한 데이터베이스를 키나 비밀번호 없이 열면 `KEY_REQUIRED`, 다른 키로 열면 `WRONG_KEY`로 실패합니다. 평문 데이터베이스를 키로 열면 `INVALID_ARGUMENT`로 실패하며, 평문 데이터베이스가 암호화되는 일은 없습니다. 암호화하려면 새 파일이 필요합니다.
- **비밀은 하나.** `key`와 [`password`](#password)는 서로를 대신합니다. 마지막에 준 것이 쓰입니다.

키는 운영체제의 키 저장소처럼 잃어버리지 않을 곳에 보관하세요. 키가 없으면 데이터를 되찾을 수 없습니다. 자세한 내용은 [암호화](../../guide/encryption.md)에 있습니다.

### password

```rust
pub fn password(&mut self, password: impl AsRef<[u8]>) -> &mut Self
```

새 데이터베이스를 `password`에서 만든 키로 암호화하거나, 암호화한 데이터베이스를 그 비밀번호로 엽니다. 비밀번호는 [`password_hashing`](#password-hashing)이 정한 비용으로 Argon2id 해시를 거쳐 데이터 키를 감싸는 키가 됩니다. 나머지는 [`key`](#key)와 같습니다. 빈 비밀번호를 주면 `open`이 `INVALID_ARGUMENT`로 실패합니다.

### password_hashing

```rust
pub fn password_hashing(
    &mut self,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> &mut Self
```

새 데이터베이스를 비밀번호로 암호화하거나 [`Database::set_password`](./database.md#set-password)로 비밀번호를 바꿀 때, 비밀번호 해시에 드는 비용을 정합니다. 인자는 차례로 Argon2id의 메모리(KiB 단위), 반복 횟수, 병렬도입니다. 기본값은 요즘 컴퓨터에서 수십 밀리초가 걸리고 모바일 앱 확장의 메모리 한도 안에 들어갑니다. 비용을 올리면 공격자가 비밀번호를 추측하기 어려워지는 대신 데이터베이스를 여는 시간도 늘어납니다.

파일에는 만들 때 쓴 비용이 기록되므로, 파일을 열 때는 이 옵션과 상관없이 기록된 비용으로 해시합니다. 메모리는 레인마다 8 KiB부터 1 GiB까지, 반복 횟수는 1부터 1024까지, 병렬도는 1부터 64까지 받습니다. 그 밖의 값을 주면 `open`이 `INVALID_ARGUMENT`로 실패합니다.

### schema

```rust
pub fn schema(&mut self, schema: Schema) -> &mut Self
```

데이터베이스의 컬렉션과 그 객체가 담는 내용을 버전과 함께 선언합니다. [`Schema`](./schema.md)를 보세요. 처음 열 때 스키마를 파일에 저장합니다. 그 뒤로 같은 버전인 파일은 스키마가 같으면 열리고 다르면 `SCHEMA_MISMATCH`로 실패하므로, 스키마를 바꾸려면 버전을 올려야 합니다. 더 낮은 버전을 가진 파일은 `open`이 반환하기 전에 마이그레이션하고, 더 높은 버전을 가진 파일은 `SCHEMA_TOO_NEW`로 실패합니다. 스키마 없이 연 데이터베이스에는 컬렉션이 없고 바이트 트리만 쓸 수 있습니다.

스키마를 쓰는 예는 [컬렉션과 객체](../../guide/objects.md)에 있습니다.

### migration

```rust
pub fn migration(&mut self, migration: Migration) -> &mut Self
```

[`Migration`](./migration.md)을 더합니다. `Migration`에는 엔진이 알아서 하는 변경 말고, 스키마 버전 하나가 이전 버전에서 바꾸는 내용을 적습니다. 더 낮은 버전을 가진 파일을 열면 선언한 버전까지의 마이그레이션을 버전 순서대로, 쓰기 트랜잭션 하나 안에서 실행합니다. 2보다 낮거나 선언한 버전보다 높은 버전으로 가는 마이그레이션을 주거나, 스키마 없이 마이그레이션만 주면 `open`이 `INVALID_ARGUMENT`로 실패합니다.

### open

```rust
pub fn open(&self, path: impl AsRef<Path>) -> Result<Database>
```

이 옵션으로 `path`에 있는 데이터베이스를 엽니다. 아무것도 없고 `create`가 허락하면 먼저 새로 만듭니다. 스키마를 줬다면 반환하기 전에 스키마를 저장하거나, 확인하거나, 마이그레이션합니다.

- **`INVALID_ARGUMENT`**: 범위를 벗어난 옵션, 저장할 수 없는 스키마나 마이그레이션, 평문 파일에 준 키.
- **`NOT_FOUND`**: 경로에 아무것도 없는데 `create(false)`일 때.
- **`NOT_A_DATABASE`**, **`UNSUPPORTED_FORMAT_VERSION`**, **`CORRUPTED`**: DaruDB 데이터베이스가 아니거나, 다른 형식 버전이거나, 손상된 파일.
- **`KEY_REQUIRED`**, **`WRONG_KEY`**: 암호화한 파일을 키나 비밀번호 없이 열었거나, 다른 것으로 열었을 때.
- **`BUSY`**: 다른 프로세스가 바쁨 대기 시간보다 오래 파일을 복구하고 있거나, 되살리기가 파일을 읽고 있을 때. 스키마를 저장하거나 마이그레이션해야 하는데 쓰기 트랜잭션이 그만큼 오래 비지 않았을 때도 해당합니다.
- **`UNSUPPORTED_FILE_SYSTEM`**: 파일이 네트워크 파일 시스템이나 잠금이 동작하지 않는 파일 시스템에 있을 때.
- **`SCHEMA_MISMATCH`**, **`SCHEMA_TOO_NEW`**: 같은 버전인데 저장된 스키마와 다르거나, 파일의 버전이 더 높을 때.
- **마이그레이션 오류**: 새 고유 인덱스가 같은 값을 두 번 찾으면 `DUPLICATE_KEY`, 마이그레이션 함수가 돌려준 `MIGRATION_FAILED`나 그 밖의 오류. 이때 파일은 예전 스키마와 데이터를 그대로 유지합니다.
- **`IO`**: 운영체제가 파일 작업에 실패했을 때.

모든 오류 코드는 [오류](../../guide/errors.md)에서 설명합니다.

### open_migrating

```rust
pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening>
```

[`open`](#open)처럼 데이터베이스를 열되, 마이그레이션이 버전 단계 사이마다 멈추고 호출한 쪽에 차례를 넘깁니다. 언어 바인딩은 마이그레이션 함수를 Rust 클로저로 넘길 수 없어서 이렇게 엽니다. 애플리케이션은 `open`을 씁니다. 자세한 내용은 [Opening과 PendingMigration](./opening.md)에 있습니다.

### salvage

```rust
pub fn salvage(
    &self,
    from: impl AsRef<Path>,
    into: impl AsRef<Path>,
) -> Result<SalvageReport>
```

`from`에 있는 손상된 데이터베이스에서 건질 수 있는 것을 `into`의 새 데이터베이스로 되살리고, 되살린 것과 되살리지 못한 것을 [`SalvageReport`](../../types/rust/salvage-report.md)로 돌려줍니다. 파일을 데이터베이스로 열지 않고 페이지 단위로 읽으므로, 열리지 않는 파일에도 쓸 수 있습니다. 이 옵션 가운데서는 암호화한 파일의 키나 비밀번호와 바쁨 대기 시간만 씁니다.

파일에 기록된 가장 최근 커밋에서 시작하고, 그 커밋으로 읽지 못한 부분은 파일에 아직 남아 있는 같은 페이지의 이전 버전에서 가져옵니다. 새 파일에는 객체에서 모든 인덱스를 다시 만들어 넣으므로 무결성 검사를 통과합니다. 페이지 크기와 암호 방식, 키는 원래 파일과 같습니다.

되살리기는 파일을 혼자 써야 합니다. 이 프로세스나 다른 프로세스가 연 파일이면 `BUSY`로 실패합니다. 새 파일은 반환할 때 이미 디스크에 기록돼 있고, `into`에 이미 있는 파일은 덮어쓰지 않고 `INVALID_ARGUMENT`로 실패합니다. 언제 되살리기를 쓰는지는 [도구](../../guide/tools.md)에 있습니다.
