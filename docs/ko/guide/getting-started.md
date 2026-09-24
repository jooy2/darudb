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

저장 커널은 이름 붙은 트리에 바이트 키와 바이트 값을 저장합니다. 모든 변경은 쓰기 트랜잭션 안에서 일어나고, `commit`이 반환될 때 한꺼번에 보이는 동시에 내구성을 갖습니다. 읽기 트랜잭션은 시작한 뒤에 무엇이 커밋되든, 살아 있는 동안 커밋 하나만 봅니다.

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

`commit` 없이 버린 쓰기 트랜잭션은 취소되고, 그 안에서 한 일은 파일에 남지 않습니다. 쓰기 트랜잭션은 한 번에 하나뿐입니다. `begin_write`는 이미 실행 중인 트랜잭션을 바쁨 대기 시간만큼 기다리며, `OpenOptions::busy_timeout`으로 바꾸지 않으면 5초입니다.

`commit`은 디스크에 기록될 때까지 기다렸다가 반환합니다. `commit_deferred`는 기다리지 않습니다. 변경은 곧바로 읽기 트랜잭션에 보이고, 다음 `commit`이나 `Database::sync`, 데이터베이스를 닫을 때, 또는 기본값으로 1초를 기다린 뒤에 이후 커밋과 함께 디스크에 기록됩니다. 프로세스가 비정상 종료돼도 하나도 잃지 않습니다. 정전이 나면 가장 최근 것부터 되돌려질 수 있지만, 중간이 빠지거나 파일이 손상되지는 않습니다. 기다릴 수 있는 양은 `OpenOptions::max_unsynced_time`과 `OpenOptions::max_unsynced_pages`로 정합니다.

파일 하나를 여러 프로세스가 동시에 열 수 있습니다. 한 프로세스가 커밋하면 다른 프로세스에 곧바로 보이고, 쓰는 쪽은 한 번에 하나이며, 읽는 쪽은 쓰는 쪽을 기다리지 않습니다. `begin_write`는 다른 프로세스의 쓰기 트랜잭션도 같은 프로세스의 것과 똑같이 바쁨 대기 시간만큼 기다립니다. 프로세스끼리는 운영체제의 파일 잠금만으로 조율하므로, 어느 순간에 프로세스가 죽어도 나머지가 뒷정리할 것은 남지 않습니다. 이 잠금과 커밋이 기다리는 동기화는 로컬 디스크에서만 제대로 동작합니다. 그래서 NFS나 SMB 같은 네트워크 파일 시스템에 있는 데이터베이스는 `UNSUPPORTED_FILE_SYSTEM`으로 거부합니다. 잠금 때문에 지켜야 할 규칙도 두 가지 있습니다. 데이터베이스를 연 프로세스 안에서는 복사하려는 목적이라도 그 파일을 따로 열면 안 됩니다. Linux와 macOS에서는 그렇게 연 핸들을 닫는 순간 데이터베이스가 쥔 잠금까지 풀립니다. 그리고 iOS 앱이 App Group 컨테이너에 데이터베이스를 두었다면, 앱이 일시 중지되기 전에 닫아야 합니다. iOS는 그곳에 잠금을 쥔 채 일시 중지된 앱을 종료합니다.

타입이 있는 레코드와 쿼리는 나중에 이 트리 위에 올라갑니다.

#### 암호화

키나 비밀번호로 만든 데이터베이스는 암호화됩니다. 키와 값, 트리 이름까지 모든 페이지가 암호화됩니다. 모든 페이지와 헤더에 기록된 커밋 정보도 인증되므로, 바이트 하나라도 바뀌면 그대로 읽지 않고 `CORRUPTED`로 알립니다.

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    db.set_password("a new password")?;
    db.close()
}
```

페이지 암호화에는 데이터베이스를 만드는 기기에서 더 빠른 쪽을 씁니다. AES 명령어가 있는 프로세서에서는 XAES-256-GCM을, 없는 곳에서는 XChaCha20-Poly1305를 씁니다. 비밀번호 대신 32바이트 키를 쓰려면 `OpenOptions::key`를 씁니다. 운영체제의 키 저장소에 보관한 키가 그런 예입니다. 비밀번호는 Argon2id로 해시하며, 기본 비용으로 수십 밀리초가 걸립니다. 이 비용은 `OpenOptions::password_hashing`으로 올리거나 내릴 수 있습니다. 키나 비밀번호를 바꿔도 페이지를 다시 암호화하지 않고, 바꾸기가 끝나면 이전 것으로는 파일을 열 수 없습니다. 평문 데이터베이스는 평문으로 남고, 암호화한 데이터베이스는 키 없이 열 수 없습니다. 키를 잃어버리지 않을 곳에 보관하세요.

### Node.js

```js
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();
```

`Database.open`의 두 번째 인자는 옵션 객체입니다. `create: false`를 주면 없는 파일을 만들지 않고, `pageSize`로 새 파일의 페이지 크기를 정합니다. Node.js 패키지에는 아직 트랜잭션이 없습니다. 엔진 API가 자리를 잡은 뒤에 들어갑니다.

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
| `BUSY` | 다른 쓰기 트랜잭션이 바쁨 대기 시간보다 오래 데이터베이스를 쥐고 있거나, 다른 프로세스가 그만큼 오래 복구하고 있을 때 |
| `SYNC_FAILED` | 파일 동기화가 실패했을 때. 마지막 커밋이 반영됐는지 알 수 없으니 파일을 다시 엽니다 |
| `KEY_REQUIRED` | 암호화한 데이터베이스를 키나 비밀번호 없이 열었을 때 |
| `WRONG_KEY` | 키나 비밀번호로 데이터베이스를 열 수 없을 때 |
| `UNSUPPORTED_FILE_SYSTEM` | 데이터베이스가 네트워크 파일 시스템이나 파일 잠금이 동작하지 않는 파일 시스템에 있을 때. 로컬 디스크에 두어야 합니다 |
| `INTERNAL` | DaruDB의 버그로만 생길 수 있는 문제가 났을 때. 제보해 주세요 |
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
