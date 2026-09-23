---
title: 시작하기
order: 2
---

# 시작하기

DaruDB는 아직 배포 전이므로, 지금은 소스에서 빌드한 뒤 Rust나 Node.js에서 첫 데이터베이스를 열어 봅니다.

## 요구 사항

- [rustup](https://rustup.rs)으로 설치한 **Rust**. 저장소가 `rust-toolchain.toml`에 컴파일러 버전을 고정해 두었으므로, 처음 빌드할 때 `rustup`이 그 버전을 설치합니다. `darudb` 크레이트를 의존성으로 쓰는 프로그램에는 Rust 1.85 이상이 필요합니다.
- Node.js 패키지를 쓰려면 **Node.js 20 이상**. 소스에서 빌드할 때는 빌드 도구 때문에 조금 더 높은 버전이 필요합니다. 20 버전대에서는 20.17 이상, 그 밖에는 22.13 이상입니다.
- 저장소를 내려받을 **Git**.

DaruDB는 유닉스 계열 시스템과 Windows에서 동작합니다. NFS나 SMB 같은 네트워크 파일 시스템은 지원하지 않습니다. 그런 파일 시스템의 잠금과 동기화는 데이터베이스가 기대하는 보장을 지키지 않기 때문입니다.

## 소스에서 빌드하기

```bash
git clone https://github.com/jooy2/darudb.git
cd darudb
cargo test --workspace
```

`cargo test`는 엔진을 빌드하고 테스트를 돌립니다. 툴체인이 제대로 설치됐는지 가장 빨리 확인하는 방법입니다.

Node.js 패키지는 해당 폴더에서 네이티브 애드온을 빌드합니다.

```bash
cd packages/node
npm install
npm run build
```

`npm run build`는 엔진과 바인딩을 지금 플랫폼용 애드온 하나로 컴파일하고, 그 옆에 `index.js`와 `index.d.ts`를 만듭니다.

## 데이터베이스 열기

데이터베이스는 파일 하나입니다. 아무것도 없는 경로를 열면 파일을 새로 만들고, 이미 있는 파일을 열면 이 버전이 읽을 수 있는 DaruDB 데이터베이스인지 확인합니다.

### Rust

배포 전까지는 경로로 크레이트를 추가합니다.

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb" }
```

```rust
use darudb::{Database, OpenOptions};

fn main() -> Result<(), darudb::Error> {
    let db = Database::open("app.darudb")?;
    println!("page size: {} bytes", db.page_size());
    db.close()?;

    // Open only if the file is already there.
    let db = OpenOptions::new().create(false).open("app.darudb")?;
    db.close()
}
```

### Node.js

```js
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();
```

`Database.open`의 두 번째 인자는 옵션 객체입니다. `create: false`를 주면 없는 파일을 만들지 않고, `pageSize`로 새 파일의 페이지 크기를 정합니다.

## 오류

모든 오류에는 실패의 종류를 나타내는 `code`가 있습니다. 코드는 Rust(`Error::code`)와 Node.js(`error.code`)에서 같고 릴리스가 바뀌어도 달라지지 않습니다. 메시지는 사람이 읽으라고 있는 것이니, 프로그램은 코드로 판단하면 됩니다.

| 코드 | 발생하는 경우 |
| --- | --- |
| `NOT_FOUND` | 경로에 아무것도 없는데 새로 만드는 것을 허용하지 않았을 때 |
| `NOT_A_DATABASE` | 파일은 있지만 DaruDB 데이터베이스가 아닐 때 |
| `UNSUPPORTED_FORMAT_VERSION` | DaruDB 데이터베이스지만 이 빌드가 읽을 수 없는 파일 형식 버전일 때 |
| `CORRUPTED` | DaruDB 데이터베이스지만 일부가 손상됐을 때 |
| `INVALID_ARGUMENT` | 옵션 값이 허용 범위를 벗어났을 때. 예를 들어 페이지 크기가 2의 거듭제곱이 아닐 때 |
| `CLOSED` | Node.js 데이터베이스 객체를 `close` 뒤에 다시 썼을 때 |
| `IO` | 운영체제가 파일 작업에 실패했을 때. 메시지에 운영체제가 보고한 내용이 들어갑니다 |

```js
try {
  Database.open('missing.darudb', { create: false });
} catch (error) {
  if (error.code === 'NOT_FOUND') {
    // Nothing exists at that path.
  }
}
```
