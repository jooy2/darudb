---
title: Database
order: 1
---

# Database

`Database`는 열린 데이터베이스 파일의 핸들이며, 모든 트랜잭션은 여기서 시작합니다.

```rust
#[derive(Debug, Clone)]
pub struct Database
```

`Database::open`으로 얻고, 옵션이 필요하면 [`OpenOptions::open`](./open-options.md#open)으로 얻습니다. 핸들을 복제하거나 같은 프로세스에서 같은 파일을 다시 열면, 공유 인스턴스 하나를 가리키는 핸들이 하나 더 생깁니다. 파일 핸들과 페이지 캐시는 하나씩이고, 쓰기 트랜잭션도 한 번에 하나입니다. 마지막 핸들을 버리면 지연 커밋을 디스크에 기록하고 파일을 닫습니다. [`close`](#close)도 같은 일을 하지만, 실패하면 알려 준다는 점이 다릅니다. `Database`는 `Send`이자 `Sync`이므로 핸들을 복제해 다른 스레드로 넘기거나 여러 스레드가 함께 써도 됩니다.

핸들은 열 때 받은 스키마를 계속 쓰고, 그 핸들의 트랜잭션은 이 스키마로 컬렉션에 접근합니다. 다른 핸들이나 다른 프로세스가 파일을 마이그레이션하면, 예전 핸들로 컬렉션에 접근하는 다음 트랜잭션은 `SCHEMA_MISMATCH`로 실패합니다. 그때는 새 스키마로 파일을 다시 열어야 합니다.

## 연관 함수

### open

```rust
pub fn open(path: impl AsRef<Path>) -> Result<Self>
```

`path`에 있는 데이터베이스를 엽니다. 아무것도 없으면 새로 만듭니다. `OpenOptions::new().open(path)`와 같으므로 실패하는 경우도 [`OpenOptions::open`](./open-options.md#open)과 같습니다.

```rust
use darudb::Database;

fn main() -> darudb::Result<()> {
    let db = Database::open("app.darudb")?;

    println!("{} bytes per page", db.page_size());
    db.close()
}
```

## 메서드

### path

```rust
pub fn path(&self) -> &Path
```

데이터베이스를 연 경로입니다. 인스턴스를 공유하는 핸들은 이 경로도 공유합니다. 프로세스에서 그 파일을 처음 열 때 쓴 경로가 나옵니다.

### page_size

```rust
pub fn page_size(&self) -> u32
```

파일의 페이지 크기를 바이트 단위로 돌려줍니다. 페이지 크기는 파일을 만들 때 [`OpenOptions::page_size`](./open-options.md#page-size)로 정해지고 바뀌지 않습니다.

### format_version

```rust
pub fn format_version(&self) -> u32
```

파일의 파일 형식 버전입니다. 빌드 하나는 한 가지 버전만 읽고 쓰며, 다른 버전의 파일은 열 때 거부합니다. 그래서 이 값은 언제나 [`FORMAT_VERSION`](../../types/rust/constants.md)입니다.

### begin_read

```rust
pub fn begin_read(&self) -> Result<ReadTransaction>
```

[읽기 트랜잭션](./read-transaction.md)을 시작합니다. 읽기 트랜잭션은 마지막 커밋을 보고, 그 뒤의 커밋은 보지 않습니다. 쓰기 트랜잭션을 기다리지 않으며, 스레드와 프로세스 수에 상관없이 읽기 트랜잭션을 몇 개든 함께 열 수 있습니다. 파일 동기화가 한 번 실패한 뒤에는 `SYNC_FAILED`로 실패합니다.

### begin_write

```rust
pub fn begin_write(&self) -> Result<WriteTransaction>
```

[쓰기 트랜잭션](./write-transaction.md)을 시작합니다. 쓰기 트랜잭션은 모든 스레드와 프로세스를 통틀어 파일마다 하나뿐입니다. 그래서 이미 실행 중인 트랜잭션이 있으면 기다리고, 바쁨 대기 시간이 지나면 `BUSY`로 실패합니다. 바쁨 대기 시간은 [`OpenOptions::busy_timeout`](./open-options.md#busy-timeout)으로 정하며 기본값은 5초입니다. 쓰기 트랜잭션을 쥔 스레드가 이 메서드를 다시 부르면 자기 자신을 기다리다가 똑같이 실패합니다. 파일 동기화가 한 번 실패한 뒤에는 `SYNC_FAILED`로 실패합니다.

### sync

```rust
pub fn sync(&self) -> Result<()>
```

지금까지의 커밋을 지연 커밋까지 모두 디스크에 기록합니다. 어느 프로세스가 한 커밋인지는 상관없습니다. 기다리는 지연 커밋이 없으면 바로 반환합니다. 있으면 이 프로세스나 다른 프로세스에서 실행 중인 쓰기 트랜잭션을 기다리고, 바쁨 대기 시간이 지나면 `BUSY`로 실패합니다. 동기화가 실패하면 `SYNC_FAILED`를 돌려주며, 그 뒤에는 파일을 다시 열어야 합니다. 지연 커밋은 [트랜잭션](../../guide/transactions.md)에서 설명합니다.

### is_encrypted

```rust
pub fn is_encrypted(&self) -> bool
```

데이터베이스가 암호화돼 있는지 알려 줍니다. 키나 비밀번호로 만든 데이터베이스가 암호화되며, 만든 뒤에는 바뀌지 않습니다.

### set_key

```rust
pub fn set_key(&self, key: [u8; 32]) -> Result<()>
```

암호화한 데이터베이스의 키를 `key`로 바꿉니다. 데이터 키를 새로 감쌀 뿐 페이지는 다시 암호화하지 않으므로, 파일 크기와 상관없이 동기 커밋 세 번이면 끝납니다. 한 번은 새 키 블록을 쓰고, 나머지 두 번은 다른 커밋 슬롯에 남은 이전 키 블록을 덮어씁니다. 반환한 뒤에는 이전 키나 비밀번호로 파일을 열 수 없습니다.

평문 데이터베이스에서는 `INVALID_ARGUMENT`로 실패합니다. 평문 데이터베이스를 암호화하려면 새 파일이 필요합니다. 쓰기 트랜잭션은 `begin_write`처럼 기다립니다. 자세한 내용은 [암호화](../../guide/encryption.md)에 있습니다.

### set_password

```rust
pub fn set_password(&self, password: impl AsRef<[u8]>) -> Result<()>
```

암호화한 데이터베이스의 키를 `password`에서 Argon2id로 만든 키로 바꿉니다. 해시 비용은 이 프로세스에서 파일을 처음 연 핸들의 [`OpenOptions::password_hashing`](./open-options.md#password-hashing)을 따릅니다. 빈 비밀번호는 `INVALID_ARGUMENT`로 실패합니다. 나머지는 [`set_key`](#set-key)와 같습니다.

### check

```rust
pub fn check(&self) -> Result<CheckReport>
```

게시된 커밋을 빠짐없이 검사합니다. 그 커밋이 닿는 모든 페이지가 부모가 기록한 검사값과 맞는지, 모든 키가 순서대로 있는지, 트리마다 항목 수가 맞는지, 파일의 모든 페이지가 사용 중이거나 비었거나 보류 중인 상태 중 정확히 하나인지 봅니다. 스키마가 있는 파일에서는 모든 객체가 인덱스와 맞는지도 봅니다. 첫 문제에서 멈추지 않고 찾은 문제를 모두 [`CheckReport`](../../types/rust/check-report.md)에 담으며, 검사를 시작할 수 없을 때만 실패합니다.

읽기 트랜잭션이 볼 커밋을 기준으로 파일 전체를 읽으므로, 검사하는 동안에도 다른 핸들과 프로세스가 쓸 수 있습니다. 무엇을 읽는지는 [도구](../../guide/tools.md)에서 설명합니다.

### backup

```rust
pub fn backup(&self, path: impl AsRef<Path>) -> Result<BackupReport>
```

게시된 커밋을 `path`에 새 파일로 복사하고 [`BackupReport`](../../types/rust/backup-report.md)를 돌려줍니다. 그동안 다른 핸들과 프로세스는 계속 읽고 씁니다. 사본에는 백업을 시작할 때 게시돼 있던 커밋이 담깁니다.

사본의 페이지 크기는 원본과 같고, 원본이 암호화돼 있으면 암호 방식과 키도 같으므로 같은 키나 비밀번호로 열립니다. 사본에는 빈 공간이 없습니다. 사본은 디스크에 기록될 때까지 `path` 옆에 임시 이름으로 있다가, 그 뒤에야 `path`로 옮겨집니다. `path`에 이미 있는 파일은 덮어쓰지 않고 `INVALID_ARGUMENT`로 실패합니다.

### backup_with

```rust
pub fn backup_with(&self, path: impl AsRef<Path>, options: &BackupOptions) -> Result<BackupReport>
```

[`backup`](#backup)처럼 사본을 쓰되 [`BackupOptions`](./backup-options.md)를 따릅니다. 옵션에 키나 비밀번호가 있으면 사본은 무작위로 만든 새 데이터 키로 암호화되고, 그 키나 비밀번호가 새 데이터 키를 감싸며, 사본은 그것으로만 열립니다. 평문 데이터베이스의 사본도 같은 방식으로 암호화됩니다. 빈 비밀번호는 아무것도 쓰기 전에 `INVALID_ARGUMENT`로 실패합니다.

### compact

```rust
pub fn compact(&self) -> Result<CompactReport>
```

파일을 그 자리에서 줄이고 [`CompactReport`](../../types/rust/compact-report.md)를 돌려줍니다. 삽입으로 페이지가 덜 찬 트리를 꽉 채워 다시 써서 파일이 백업 사본과 비슷한 크기까지 줄게 한 다음, 파일 끝쪽 페이지를 모두 앞쪽 빈 페이지로 옮기는 쓰기 트랜잭션을 따로 실행하고, 이어지는 커밋이 비게 된 끝을 파일 시스템에 돌려줍니다.

그동안 다른 핸들과 프로세스는 계속 읽고 씁니다. 다만 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 그 트랜잭션이 끝날 때까지 남습니다. 그래서 오래 도는 읽기 트랜잭션이 있으면 덜 줄어들고, 남은 부분은 다음 압축이 가져갑니다. 쓰기 트랜잭션은 `begin_write`처럼 기다립니다.

### close

```rust
pub fn close(self) -> Result<()>
```

지연 커밋을 디스크에 기록한 뒤 이 핸들을 닫습니다. 이 파일의 어느 핸들에서든 동기화가 실패한 적이 있으면 `SYNC_FAILED`로 실패합니다. 그 실패를 알아챌 마지막 기회입니다. 실행 중인 쓰기 트랜잭션을 바쁨 대기 시간보다 오래 기다리면 `BUSY`로 실패합니다. 핸들을 그냥 버려도 마지막 핸들이면 지연 커밋을 디스크에 기록하지만, 실패를 알려 줄 수는 없습니다.

### schema_record

```rust
pub fn schema_record(&self) -> Option<&[u8]>
```

이 핸들이 파일을 열 때 쓴 스키마를 파일이 저장하는 형식으로 인코딩해 돌려줍니다. 스키마 없이 연 핸들이면 `None`입니다. 언어 바인딩을 위한 메서드입니다. 바인딩은 여기서 컬렉션과 필드의 ID를 읽어 레코드를 직접 인코딩하고 디코딩합니다. 바인딩이 이를 어떻게 쓰는지는 [바인딩](../../engine/bindings.md)에, 인코딩 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-stored-schema)에 있습니다. Rust 프로그램에서는 쓸 일이 없습니다.
