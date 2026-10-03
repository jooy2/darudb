---
title: 시작하기
order: 2
---

# 시작하기

DaruDB는 아직 배포 전이므로, 지금은 소스에서 빌드하고 경로로 프로젝트에 추가한 뒤 첫 데이터베이스를 열어 봅니다.

## 요구 사항

::: lang rust

- [rustup](https://rustup.rs)으로 설치한 **Rust**. 저장소가 `rust-toolchain.toml`에 컴파일러 버전을 고정해 두었으므로, 처음 빌드할 때 `rustup`이 그 버전을 설치합니다. `darudb` 크레이트를 의존성으로 쓰는 프로그램에는 Rust 1.85 이상이 필요합니다.

:::

::: lang node

- **Node.js 20 이상**. 소스에서 빌드할 때는 빌드 도구 때문에 조금 더 높은 버전이 필요합니다. 20 버전대에서는 20.17 이상, 그 밖에는 22.13 이상입니다.
- [rustup](https://rustup.rs)으로 설치한 **Rust**. 패키지의 네이티브 애드온을 엔진에서 컴파일하기 때문입니다. 저장소가 `rust-toolchain.toml`에 컴파일러 버전을 고정해 두었으므로, 처음 빌드할 때 `rustup`이 그 버전을 설치합니다.

:::

- 저장소를 내려받을 **Git**.

DaruDB는 유닉스 계열 시스템과 Windows에서 동작합니다. NFS나 SMB 같은 네트워크 파일 시스템은 지원하지 않습니다. 그런 파일 시스템의 잠금과 동기화는 데이터베이스가 기대하는 보장을 지키지 않기 때문입니다.

## 소스에서 빌드하기

```bash
git clone https://github.com/jooy2/darudb.git
cd darudb
```

::: lang rust

```bash
cargo test -p darudb
```

`cargo test`는 엔진을 빌드하고 테스트를 돌립니다. 툴체인이 제대로 설치됐는지 가장 빨리 확인하는 방법입니다.

:::

::: lang node

```bash
cd packages/node
npm install
npm run build
```

`npm run build`는 엔진과 바인딩을 지금 플랫폼용 애드온 하나로 컴파일하고, 애드온을 불러오는 파일을 만들고, TypeScript API를 `dist/`로 컴파일합니다. 그다음 `npm test`를 실행하면 방금 빌드한 결과로 패키지 테스트를 돌립니다.

:::

## 프로젝트에 추가하기

::: lang rust

배포 전까지는 경로로 크레이트를 추가합니다.

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb" }
```

:::

::: lang node

빌드한 폴더를 설치하면 프로젝트에 링크로 연결됩니다.

```bash
npm install ../darudb/packages/node
```

패키지는 TypeScript를 먼저 생각해 만들었습니다. 타입 선언이 함께 들어 있고, 객체의 타입은 선언한 스키마에서 나옵니다. JavaScript에서도 그대로 쓸 수 있습니다.

:::

## 데이터베이스 열기

데이터베이스는 파일 하나입니다. 아무것도 없는 경로를 열면 파일을 새로 만들고, 이미 있는 파일을 열면 이 빌드가 읽을 수 있는 DaruDB 데이터베이스인지 확인합니다.

::: lang rust

```rust
use darudb::{Database, OpenOptions};

fn main() -> Result<(), darudb::Error> {
    let db = Database::open("app.darudb")?;
    println!("page size: {} bytes", db.page_size());
    db.close()?;

    // 파일이 이미 있을 때만 엽니다.
    let db = OpenOptions::new().create(false).open("app.darudb")?;
    db.close()
}
```

:::

::: lang node

```ts
import { Database } from 'darudb';

const db = Database.open('app.darudb');

console.log(`page size: ${db.pageSize} bytes`);
db.close();

// 파일이 이미 있을 때만 엽니다.
Database.open('app.darudb', { create: false }).close();
```

:::

## 첫 객체 저장하기

스키마에는 데이터베이스가 담을 컬렉션과 그 객체의 필드를 적습니다. 스키마를 주고 데이터베이스를 연 뒤, 쓰기 트랜잭션에서 객체를 쓰고 읽기 트랜잭션에서 다시 찾습니다.

::: lang rust

```rust
use darudb::{Collection, Filter, Object, OpenOptions, Query, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1).collection(
        Collection::new("users")
            .field("name", Type::String)
            .with_default("age", Type::Int, 0)
            .index("age"),
    );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;
    users.insert(Object::new().with("name", "Alice").with("age", 31))?;
    users.insert(Object::new().with("name", "Bob").with("age", 17))?;
    txn.commit()?;

    let read = db.begin_read()?;
    let adults = read
        .collection("users")?
        .query(&Query::new().filter(Filter::ge("age", 18)))?;
    println!("{adults:?}");

    db.close()
}
```

:::

::: lang node

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    name: t.string(),
    age: t.int().default(0).index()
  })
});

const db = Database.open('app.darudb', { schema: app });

db.write((txn) => {
  const users = txn.collection('users');

  users.insert({ name: 'Alice', age: 31 });
  users.insert({ name: 'Bob', age: 17 });
});

const adults = db.read((txn) => txn.collection('users').find((q) => q.where('age', '>=', 18)));

console.log(adults); // [{ id: 1, name: 'Alice', age: 31 }]
db.close();
```

:::

이 컬렉션에는 기본 키 필드가 없으므로 엔진이 객체마다 1부터 번호를 매긴 `id`를 줍니다. `age`에 인덱스가 있으므로 쿼리는 모든 객체를 읽지 않고 찾는 객체만 읽습니다.

## 옵션

::: lang rust

여는 방법은 모두 `OpenOptions`로 정합니다. 메서드마다 빌더를 돌려주고, 마지막에 `open`으로 파일을 엽니다.

- `create(false)`를 주면 없는 파일을 만들지 않고 `NOT_FOUND`로 실패합니다.
- `page_size`는 새 파일의 페이지 크기입니다. 4096부터 65536 사이의 2의 거듭제곱이며 기본값은 4096입니다.
- `cache_size`는 페이지 캐시가 쓸 메모리를 바이트 단위로 정합니다. 기본값은 32 MiB입니다.
- `busy_timeout`은 쓰기가 다른 쓰기를 기다리다 `BUSY`로 실패하기까지의 시간입니다. 기본값은 5초입니다.
- `max_unsynced_pages`와 `max_unsynced_time`은 지연 커밋이 동기화하지 않고 쌓아 둘 수 있는 양을 정합니다. [트랜잭션](./transactions.md)을 보세요.
- `key`와 `password`, `password_hashing`은 새 파일을 암호화하거나 암호화한 파일을 엽니다. [암호화](./encryption.md)를 보세요.
- `schema`와 `migration`은 컬렉션과, 예전 스키마가 지금 스키마로 넘어오는 방법을 선언합니다. [컬렉션과 객체](./objects.md)와 [마이그레이션](./migrations.md)을 보세요.

옵션마다 자세한 설명은 API 섹션의 [`OpenOptions`](../api/rust/open-options.md)에 있습니다.

:::

::: lang node

`Database.open`의 두 번째 인자는 옵션 객체입니다.

- `create: false`를 주면 없는 파일을 만들지 않고 `NOT_FOUND`로 실패합니다.
- `pageSize`는 새 파일의 페이지 크기입니다. 4096부터 65536 사이의 2의 거듭제곱이며 기본값은 4096입니다.
- `cacheSize`는 페이지 캐시가 쓸 메모리를 바이트 단위로 정합니다. 기본값은 32 MiB입니다.
- `busyTimeout`은 쓰기가 다른 쓰기를 기다리다 `BUSY`로 실패하기까지의 시간을 밀리초로 정합니다. 기본값은 5000입니다.
- `key`와 `password`, `passwordHashing`은 새 파일을 암호화하거나 암호화한 파일을 엽니다. [암호화](./encryption.md)를 보세요.
- `schema`와 `migrations`는 컬렉션과, 예전 스키마가 지금 스키마로 넘어오는 방법을 선언합니다. [컬렉션과 객체](./objects.md)와 [마이그레이션](./migrations.md)을 보세요.

옵션마다 자세한 설명은 타입 섹션의 [`OpenOptions`](../types/node/open-options.md)에 있습니다.

:::

## 다음 단계

- [컬렉션과 객체](./objects.md)에서 스키마를 선언하고 객체를 읽고 씁니다.
- [쿼리](./queries.md)에서 필드로 객체를 찾는 방법을 코드와 문자열 두 가지로 봅니다.
- [트랜잭션](./transactions.md)에서 커밋이 무엇을 보장하는지, 언제 커밋을 미루면 되는지 설명합니다.
- [오류](./errors.md)에 모든 오류 코드가 있습니다.
- [마이그레이션](../migration/index.md)에서 지금 쓰는 임베디드 데이터베이스의 데이터를 DaruDB로 옮깁니다.
