---
title: 시작하기
order: 2
---

# 시작하기

DaruDB를 프로젝트에 추가하고 첫 데이터베이스를 열어 봅니다. Node.js와 Dart 패키지는 각 레지스트리에서 설치하고, 아직 배포하지 않은 Rust 크레이트는 저장소를 내려받아 씁니다.

## 요구 사항

::: lang rust

- [rustup](https://rustup.rs)으로 설치한 **Rust**. 저장소가 `rust-toolchain.toml`에 컴파일러 버전을 고정해 두었으므로, 처음 빌드할 때 `rustup`이 그 버전을 설치합니다. `darudb` 크레이트를 의존성으로 쓰는 프로그램에는 Rust 1.85 이상이 필요합니다.
- 저장소를 내려받을 **Git**.

:::

::: lang node

- **Node.js 20 이상**. macOS, Windows, Linux(glibc와 musl), FreeBSD, Android용 애드온을 미리 빌드해 패키지에 담았으므로, 설치할 때 컴파일하는 것이 없습니다.

:::

::: lang dart

- **Dart 3.10 이상**, 또는 Flutter 3.38 이상. 패키지가 빌드 훅으로 네이티브 라이브러리를 마련하는데, 빌드 훅은 이 버전부터 정식 기능입니다.
- 애플리케이션을 어떤 대상으로 처음 빌드할 때의 **네트워크 연결**. 빌드 훅이 그 대상용으로 미리 빌드한 엔진을 패키지의 GitHub 릴리스에서 내려받아 SHA-256 해시를 확인하고 캐시에 두므로, 빌드에 Rust 툴체인이 필요 없습니다. 라이브러리는 Android, iOS, macOS, Windows, Linux용이 있습니다. 정적 라이브러리가 필요하거나 미리 빌드한 라이브러리가 없는 대상이라면 git으로 패키지를 의존합니다. 그러면 빌드 훅이 [rustup](https://rustup.rs)으로 엔진을 소스에서 빌드합니다.

:::

DaruDB는 유닉스 계열 시스템과 Windows에서 동작합니다. NFS나 SMB 같은 네트워크 파일 시스템은 지원하지 않습니다. 그런 파일 시스템의 잠금과 동기화는 데이터베이스가 기대하는 보장을 지키지 않기 때문입니다.

## 프로젝트에 추가하기

::: lang rust

크레이트는 아직 배포 전이라 먼저 저장소를 내려받습니다. 엔진 테스트를 돌려 보면 툴체인이 제대로 설치됐는지 가장 빨리 확인할 수 있습니다.

```bash
git clone https://github.com/jooy2/darudb.git
cd darudb
cargo test -p darudb
```

그다음 경로로 크레이트를 프로젝트에 추가합니다.

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb" }
```

`derive` 기능을 켜면 구조체를 컬렉션의 객체로 만드는 `#[derive(Object)]`를 쓸 수 있습니다. [컬렉션과 객체](./objects.md#객체를-rust-타입으로)를 보세요.

:::

::: lang node

```bash
npm install darudb
```

패키지는 TypeScript를 먼저 생각해 만들었습니다. 타입 선언이 함께 들어 있고, 객체의 타입은 선언한 스키마에서 나옵니다. JavaScript에서도 그대로 쓸 수 있습니다.

:::

::: lang dart

패키지를 추가하고, 클래스의 코드를 써 주는 생성기도 함께 추가합니다.

```yaml
dependencies:
  darudb: ^1.0.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.0.0
```

Flutter 앱과 Dart 서버, 명령줄 도구에서 똑같이 쓸 수 있습니다. 생성기가 읽는 클래스는 [컬렉션과 객체](./objects.md)에 있습니다.

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

이 페이지의 예제는 `await` 없이 결과를 바로 돌려주는 동기 API를 씁니다. 파일을 쓰는 호출마다 이름이 `Async`로 끝나고 promise를 돌려주는 짝도 있습니다. 서버나 Electron의 메인 프로세스에서는 디스크를 기다리는 동안 이벤트 루프가 멈추지 않도록 쓰기와 파일 열기에 이쪽을 쓰세요. 어디에 무엇을 쓸지는 [동기와 비동기 고르기](./async.md#동기와-비동기-고르기)에 있습니다.

:::

::: lang dart

```dart
import 'package:darudb/darudb.dart';

void main() {
  final db = Database.open('app.darudb');

  print('page size: ${db.pageSize} bytes');
  db.close();

  // 파일이 이미 있을 때만 엽니다.
  Database.open('app.darudb', create: false).close();
}
```

이 페이지의 예제는 `await` 없이 결과를 바로 돌려주는 동기 API를 씁니다. 파일을 쓰는 호출마다 이름이 `Async`로 끝나고 `Future`를 돌려주는 짝도 있습니다. Flutter 앱의 UI isolate에서는 디스크를 기다리는 동안 프레임이 밀리지 않도록 쓰기와 파일 열기에 이쪽을 쓰세요. 어디에 무엇을 쓸지는 [동기와 비동기 고르기](./async.md#동기와-비동기-고르기)에 있습니다.

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

::: lang dart

```dart
import 'package:darudb/darudb.dart';

part 'main.g.dart';

@Collection('users')
class User {
  const User({this.id, required this.name, this.age = 0});

  final int? id;
  final String name;
  @Index()
  final int age;
}

void main() {
  final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

  db.write((txn) {
    final users = txn.collection(userSchema);

    users.insert(const User(name: 'Alice', age: 31));
    users.insert(const User(name: 'Bob', age: 17));
  });

  final adults = db.read(
    (txn) => txn.collection(userSchema).find((q) => q.where(q.age.atLeast(18))),
  );

  print(adults.map((user) => user.name)); // (Alice)
  db.close();
}
```

`dart run build_runner build`가 `main.g.dart`를 씁니다. 그 안에 `userSchema`와, 쿼리에서 쓴 `q.age`가 있는 쿼리 빌더가 들어 있습니다.

Dart 3.10에서는 `build_runner`가 `'dart compile' does not support build hooks`라는 오류를 내고 멈춥니다. Dart 3.10에서 쓸 수 있는 가장 새 `build_runner`는 빌더를 `dart compile`로 미리 컴파일하는데, Dart 3.10은 `darudb`처럼 빌드 훅이 있는 패키지가 든 프로젝트를 이 방식으로 컴파일하지 않기 때문입니다. 이때는 `dart run build_runner build --force-jit`으로 실행하세요. 빌더를 미리 컴파일하지 않고 실행할 때 컴파일합니다.

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

::: lang dart

`Database.open`은 이름 있는 인자로 옵션을 받습니다.

- `create: false`를 주면 없는 파일을 만들지 않고 `NOT_FOUND`로 실패합니다.
- `pageSize`는 새 파일의 페이지 크기입니다. 4096부터 65536 사이의 2의 거듭제곱이며 기본값은 4096입니다.
- `cacheSize`는 페이지 캐시가 쓸 메모리를 바이트 단위로 정합니다. 기본값은 32 MiB입니다.
- `busyTimeout`은 쓰기가 다른 쓰기를 기다리다 `BUSY`로 실패하기까지의 시간을 `Duration`으로 정합니다. 기본값은 5초입니다.
- `key`와 `password`, `passwordHashing`은 새 파일을 암호화하거나 암호화한 파일을 엽니다. [암호화](./encryption.md)를 보세요.
- `schema`와 `migrations`는 컬렉션과, 예전 스키마가 지금 스키마로 넘어오는 방법을 선언합니다. [컬렉션과 객체](./objects.md)와 [마이그레이션](./migrations.md)을 보세요.

옵션마다 자세한 설명은 API 섹션의 [`Database.open`](../api/dart/database.md#open)에 있고, `Database.openAsync`도 같은 옵션을 받습니다.

:::

## 다음 단계

- [컬렉션과 객체](./objects.md)에서 스키마를 선언하고 객체를 읽고 씁니다.
- [쿼리](./queries.md)에서 필드로 객체를 찾는 방법을 코드와 문자열 두 가지로 봅니다.
- [트랜잭션](./transactions.md)에서 커밋이 무엇을 보장하는지, 언제 커밋을 미루면 되는지 설명합니다.
- [오류](./errors.md)에 모든 오류 코드가 있습니다.
- [다른 DB에서 옮기기](../migration/index.md)에서 지금 쓰는 임베디드 데이터베이스의 데이터를 DaruDB로 옮깁니다.
