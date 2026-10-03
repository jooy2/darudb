---
title: Error와 Result
order: 4
counterpart: /types/node/error
---

# Error와 Result

`Error`는 이 크레이트에서 실패할 수 있는 모든 호출이 돌려주는 오류이고, `Result`는 그 오류를 쓰는 크레이트의 결과 타입입니다.

```rust
#[derive(Debug)]
#[non_exhaustive]
pub enum Error

pub type Result<T, E = Error> = std::result::Result<T, E>;
```

배리언트마다 [`code`](#code)가 돌려주는 고정된 코드가 있습니다. 코드는 모든 언어 바인딩에서 같은 문자열이고, 한 번 릴리스하면 이름을 바꾸지 않습니다. 실패에 대응하는 프로그램은 배리언트나 코드로 판단해야 합니다. `Display`가 쓰는 메시지는 사람이 읽으라고 있는 것이라 릴리스마다 문구가 바뀔 수 있습니다. 코드마다 언제 생기고 어떻게 대처하는지는 [오류](../../guide/errors.md)에 있습니다.

`darudb::Result<T>`는 `std::result::Result<T, Error>`입니다. 두 번째 타입 매개변수가 있어서 다른 오류 타입을 돌려주는 함수도 이 별칭을 쓸 수 있습니다.

`Error`에는 `#[non_exhaustive]`가 붙어 있어 어느 릴리스에서든 배리언트가 늘 수 있습니다. 그래서 `Error`를 `match`할 때는 와일드카드 갈래가 있어야 합니다. `std::error::Error`를 구현하고 `Send`와 `Sync`도 만족하므로, `?`로 `Box<dyn std::error::Error + Send + Sync>`나 `From<darudb::Error>`를 구현한 애플리케이션의 오류 타입으로 바꿀 수 있습니다.

```rust
use darudb::{Database, Error};

fn open_now(path: &str) -> darudb::Result<Option<Database>> {
    match Database::open(path) {
        Ok(db) => Ok(Some(db)),
        Err(Error::Busy { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}
```

## 배리언트

### Io

```rust
Io { path: PathBuf, source: io::Error }
```

`IO`. 운영체제가 파일이나 그 디렉터리를 다루다 실패했습니다. `path`는 작업 대상이고, `source`는 운영체제가 보고한 오류입니다.

### NotFound

```rust
NotFound { path: PathBuf }
```

`NOT_FOUND`. 경로에 데이터베이스가 없고, 옵션이 새로 만드는 것을 허용하지 않았습니다.

### NotADatabase

```rust
NotADatabase { path: PathBuf }
```

`NOT_A_DATABASE`. 파일이 비어 있거나, 헤더를 담기에 너무 짧거나, DaruDB의 바이트로 시작하지 않습니다.

### UnsupportedFormatVersion

```rust
UnsupportedFormatVersion { path: PathBuf, found: u32, supported: u32 }
```

`UNSUPPORTED_FORMAT_VERSION`. DaruDB 데이터베이스지만 이 빌드가 읽지 못하는 형식입니다. `found`는 파일에 기록된 버전이고 `supported`는 이 빌드가 읽는 버전입니다. 두 값은 파일 형식 버전인 [`FORMAT_VERSION`](./constants.md#format-version)이거나, 파일에 저장된 스키마의 객체 형식 버전입니다.

### Corrupted

```rust
Corrupted { path: PathBuf, reason: String }
```

`CORRUPTED`. 파일에 있을 수 없는 내용이 기록돼 있어, 파일 일부가 손상됐다고 봅니다. `reason`은 무엇을 찾았는지 알려 줍니다.

### InvalidArgument

```rust
InvalidArgument { message: String }
```

`INVALID_ARGUMENT`. 호출한 쪽이 할 수 없는 일을 요청했습니다. 범위를 벗어난 옵션, 규칙을 어긴 스키마, 컬렉션에 맞지 않는 객체, 도구가 덮어쓰지 않는 경로 같은 경우입니다.

### Closed

```rust
Closed
```

`CLOSED`. 닫은 데이터베이스를 다시 썼습니다. [`Database::close`](../../api/rust/database.md#close)가 핸들을 소비하므로 Rust API는 이 오류를 돌려주지 않습니다. 닫은 뒤에도 핸들이 남는 바인딩이 그 경우를 이 목록의 코드로 알리도록 둔 배리언트입니다.

### Busy

```rust
Busy { path: PathBuf }
```

`BUSY`. 데이터베이스가 바쁨 대기 시간보다 오래 바빴습니다. 이 프로세스나 다른 프로세스의 쓰기 트랜잭션이 쥐고 있었거나, 다른 프로세스가 복구하고 있었던 경우입니다. 열려 있는 파일을 되살리려 할 때와, 되살리는 중인 파일을 열려고 할 때도 이 오류가 납니다.

### SyncFailed

```rust
SyncFailed { path: PathBuf, source: Option<io::Error> }
```

`SYNC_FAILED`. 동기화가 실패해서 커밋이 반영됐는지 알 수 없습니다. 그 뒤로는 이 프로세스에서 그 파일을 연 모든 핸들이 이 오류로 실패하므로, 핸들이 모두 사라진 뒤 파일을 다시 열어야 합니다. `source`는 실패한 그때 운영체제가 보고한 오류이고, 그 뒤에 데이터베이스를 쓸 때는 `None`입니다.

### KeyRequired

```rust
KeyRequired { path: PathBuf }
```

`KEY_REQUIRED`. 암호화한 데이터베이스를 키나 비밀번호 없이 열었습니다.

### WrongKey

```rust
WrongKey { path: PathBuf }
```

`WRONG_KEY`. 그 키나 비밀번호로는 데이터베이스를 열 수 없습니다.

### UnsupportedFileSystem

```rust
UnsupportedFileSystem { path: PathBuf }
```

`UNSUPPORTED_FILE_SYSTEM`. 데이터베이스가 네트워크 파일 시스템이나 파일 잠금이 동작하지 않는 파일 시스템에 있습니다. 데이터베이스는 로컬 디스크에 두어야 합니다.

### SchemaMismatch

```rust
SchemaMismatch { message: String }
```

`SCHEMA_MISMATCH`. 선언한 스키마가 같은 버전으로 파일에 저장된 스키마와 다르거나, 이 핸들을 연 뒤에 다른 핸들이나 프로세스가 파일을 마이그레이션했습니다. `message`는 무엇이 다른지 알려 줍니다.

### SchemaTooNew

```rust
SchemaTooNew { stored: u64, declared: u64 }
```

`SCHEMA_TOO_NEW`. 파일에 저장된 스키마 버전 `stored`가 애플리케이션이 선언한 버전 `declared`보다 높습니다.

### DuplicateKey

```rust
DuplicateKey { message: String }
```

`DUPLICATE_KEY`. 넣으려는 기본 키가 이미 있거나, 고유 인덱스에 같은 값이 이미 있습니다. `message`에는 어느 컬렉션의 어떤 키나 값인지 적혀 있습니다.

### InvalidQuery

```rust
InvalidQuery { message: String }
```

`INVALID_QUERY`. 쿼리를 해석할 수 없거나 쿼리가 스키마에 맞지 않습니다. `message`에는 어디가 어떻게 틀렸는지 적혀 있습니다.

### MigrationFailed

```rust
MigrationFailed { message: String }
```

`MIGRATION_FAILED`. 마이그레이션 함수가 오류를 알렸습니다. 엔진은 이 오류를 만들지 않습니다. 마이그레이션 함수가 멈춘 이유를 알리려고 직접 돌려주는 오류이고, 함수가 `?`로 그대로 넘긴 엔진의 오류는 원래 코드를 유지합니다. 어느 쪽이든 파일은 예전 스키마와 데이터를 그대로 유지합니다.

```rust
use darudb::{Error, Migration};

let migration = Migration::to(2).run(|migrating| {
    if migrating.collection("users")?.len()? > 1_000_000 {
        return Err(Error::MigrationFailed {
            message: "too many users to migrate at startup".to_owned(),
        });
    }

    Ok(())
});
```

### Internal

```rust
Internal { message: String }
```

`INTERNAL`. 엔진의 불변 조건이 깨졌습니다. DaruDB의 버그로만 생기는 일이니 `message`와 함께 제보해 주세요.

## 메서드

### code

```rust
pub fn code(&self) -> &'static str
```

실패를 프로그램이 알아보도록 붙인 고정된 이름으로, `SCREAMING_SNAKE_CASE`로 씁니다. 위에서 배리언트마다 적은 코드를 돌려줍니다. Node.js 패키지는 같은 문자열을 오류의 `code`로 내놓습니다.

```rust
use darudb::{Object, WriteTransaction};

fn add_tag(txn: &mut WriteTransaction, name: &str) -> darudb::Result<bool> {
    match txn.collection("tags")?.insert(Object::new().with("name", name)) {
        Ok(_) => Ok(true),
        Err(error) if error.code() == "DUPLICATE_KEY" => Ok(false),
        Err(error) => Err(error),
    }
}
```

## 트레이트

- **`Display`.** 사람이 읽을 메시지를 씁니다. `path`가 있는 배리언트는 메시지에 파일 경로가 들어갑니다.
- **`std::error::Error`.** `Io`와, 운영체제의 오류가 있는 `SyncFailed`는 그 오류를 `source`로 돌려줍니다. 다른 배리언트에는 `source`가 없습니다.
- **`Debug`.** 배리언트를 필드와 함께 출력합니다.
