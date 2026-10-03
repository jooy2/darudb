---
title: 오류
order: 10
---

# 오류

DaruDB가 돌려주는 모든 오류에는 실패의 종류를 나타내는 `code`가 있고, 코드는 모든 언어에서 같으며 릴리스가 바뀌어도 달라지지 않습니다.

## 코드 읽기

::: lang rust

실패할 수 있는 호출은 모두 `darudb::Result`를 돌려주고, 그 오류는 `darudb::Error`입니다. `Error::code`는 코드를 문자열로 돌려주며, 배리언트는 평소처럼 `match`로 구분합니다.

```rust
use darudb::{Error, OpenOptions};

fn open_existing() -> Result<(), Error> {
    match OpenOptions::new().create(false).open("missing.darudb") {
        Ok(db) => db.close(),
        Err(error) if error.code() == "NOT_FOUND" => {
            // 경로에 아무것도 없습니다.
            Ok(())
        }
        Err(error) => Err(error),
    }
}
```

배리언트 목록은 타입 섹션의 [`Error`](../types/rust/error.md)에 있습니다.

:::

::: lang node

패키지가 던지는 모든 오류는 `code`가 아래 코드 중 하나인 `Error`입니다. 비동기 API는 같은 오류로 promise를 거부합니다.

```ts
try {
  Database.open('missing.darudb', { create: false });
} catch (error) {
  if (error.code === 'NOT_FOUND') {
    // 경로에 아무것도 없습니다.
  }
}
```

자세한 내용은 타입 섹션의 [`Error`](../types/node/error.md)에 있습니다.

:::

::: lang dart

패키지가 데이터베이스에 대해 던지는 오류는 모두 `DaruException`이고, `code`는 아래 코드 중 하나입니다. `Future` API도 같은 오류로 끝납니다.

```dart
try {
  Database.open('missing.darudb', create: false);
} on DaruException catch (error) {
  if (error.code == 'NOT_FOUND') {
    // 경로에 아무것도 없습니다.
  }
}
```

트랜잭션이나 마이그레이션 안에서 내 함수가 던진 오류는 그대로 다시 던집니다. 자세한 내용은 타입 섹션의 [`DaruException`](../types/dart/error.md)에 있습니다.

:::

메시지는 사람이 읽으라고 있는 것이고 바뀔 수 있으니, 프로그램은 코드로 판단하면 됩니다.

## 모든 코드

| 코드 | 발생하는 경우 |
| --- | --- |
| `NOT_FOUND` | 경로에 아무것도 없는데 새로 만드는 것을 허용하지 않았을 때 |
| `NOT_A_DATABASE` | 파일은 있지만 DaruDB 데이터베이스가 아닐 때 |
| `UNSUPPORTED_FORMAT_VERSION` | DaruDB 데이터베이스지만 이 빌드가 읽을 수 없는 형식 버전일 때. 첫 릴리스 전까지는 이 빌드의 형식이 아니면 더 오래됐든 더 새롭든 모두 해당합니다 |
| `CORRUPTED` | DaruDB 데이터베이스지만 일부가 손상됐을 때 |
| `INVALID_ARGUMENT` | 옵션 값이 허용 범위를 벗어났거나 객체가 스키마에 맞지 않을 때. 예를 들어 페이지 크기가 2의 거듭제곱이 아닐 때 |
| `CLOSED` | 데이터베이스나 트랜잭션, 컬렉션을 닫은 뒤나 트랜잭션이 끝난 뒤에 다시 썼을 때 |
| `BUSY` | 다른 쓰기 트랜잭션이 바쁨 대기 시간보다 오래 데이터베이스를 쥐고 있거나, 다른 프로세스가 그만큼 오래 복구하고 있을 때. 열려 있는 파일을 되살리려 하거나, 되살리는 중인 파일을 열려고 할 때도 해당합니다 |
| `SYNC_FAILED` | 파일 동기화가 실패했을 때. 마지막 커밋이 반영됐는지 알 수 없으니 파일을 다시 엽니다 |
| `KEY_REQUIRED` | 암호화한 데이터베이스를 키나 비밀번호 없이 열었을 때 |
| `WRONG_KEY` | 키나 비밀번호로 데이터베이스를 열 수 없을 때 |
| `UNSUPPORTED_FILE_SYSTEM` | 데이터베이스가 네트워크 파일 시스템이나 파일 잠금이 동작하지 않는 파일 시스템에 있을 때. 로컬 디스크에 두어야 합니다 |
| `SCHEMA_MISMATCH` | 선언한 스키마가 같은 버전으로 파일에 저장된 스키마와 다르거나, 이 핸들을 연 뒤에 파일이 마이그레이션됐을 때 |
| `SCHEMA_TOO_NEW` | 파일에 저장된 스키마 버전이 선언한 것보다 높을 때. 더 새로운 애플리케이션이 쓴 파일입니다 |
| `DUPLICATE_KEY` | 넣으려는 객체의 기본 키가 이미 있거나, 고유 인덱스에 같은 값이 이미 있을 때 |
| `INVALID_QUERY` | 쿼리가 컬렉션에 없는 필드를 쓰거나, 필드를 다른 타입의 값과 비교하거나, 문법에 맞지 않을 때 |
| `MIGRATION_FAILED` | 마이그레이션 함수가 실패를 알렸을 때. 파일은 예전 스키마와 데이터를 그대로 유지합니다 |
| `INTERNAL` | DaruDB의 버그로만 생길 수 있는 문제가 났을 때. 제보해 주세요 |
| `IO` | 운영체제가 파일 작업에 실패했을 때. 메시지에 운영체제가 보고한 내용이 들어갑니다 |
