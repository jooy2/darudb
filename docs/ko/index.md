---
layout: home

title: DaruDB
titleTemplate: 로컬 파일 하나에 담는 임베디드 데이터베이스
description: 애플리케이션의 데이터를 로컬 파일 하나에 담는 임베디드 데이터베이스입니다. Rust로 작성한 엔진 하나를 Rust와 Node.js, Dart, Python에서 함께 쓰며, 암호화와 크래시 안전성, 여러 프로세스의 동시 접근을 엔진에 담았습니다.

hero:
  name: DaruDB
  text: Rust와 Node.js, Dart, Python을 위한 임베디드 데이터베이스
  tagline: 서버 없이 로컬 파일 하나에 객체와 인덱스, 쿼리를 담습니다. Rust로 작성한 엔진 하나가 모든 언어의 일을 맡고, 암호화와 크래시 안전성, 여러 프로세스가 파일 하나를 함께 쓰는 기능도 그 엔진에 들어 있습니다.
  image:
    src: /logo.webp
    alt: DaruDB 로고
  actions:
    - theme: brand
      text: 시작하기
      link: /ko/guide/getting-started
    - theme: alt
      text: 소개
      link: /ko/guide/introduction
    - theme: alt
      text: GitHub
      link: https://github.com/jooy2/darudb

features:
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><circle cx="4.5" cy="4.5" r="2"/><circle cx="19.5" cy="4.5" r="2"/><circle cx="4.5" cy="19.5" r="2"/><circle cx="19.5" cy="19.5" r="2"/><path d="m6 6 3.8 3.8M18 6l-3.8 3.8M6 18l3.8-3.8M18 18l-3.8-3.8"/></svg>
    title: 엔진 하나, 언어 넷
    details: 엔진은 Rust로 한 번만 작성하고, Node.js와 Dart, Python은 얇은 바인딩으로 그 엔진을 씁니다. 한 언어에서 쓴 파일을 다른 언어에서 읽어도 똑같이 읽히고, 오류 코드도 모든 언어에서 같습니다.
    link: /ko/guide/introduction
    linkText: 구조 살펴보기
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="4.5" y="10.5" width="15" height="10" rx="2"/><path d="M8 10.5V7a4 4 0 0 1 8 0v3.5"/><path d="M12 14.5v2"/></svg>
    title: 페이지마다 암호화
    details: 모든 페이지를 인증 암호로 봉인합니다. 키를 직접 주거나 비밀번호에서 Argon2id로 키를 만듭니다. 키가 없으면 헤더 말고는 아무것도 보이지 않고, 키 없이 바꾼 페이지는 거부됩니다.
    link: /ko/guide/encryption
    linkText: 데이터베이스 암호화하기
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.8 5 5.5v6c0 4.4 3 7.9 7 9.2 4-1.3 7-4.8 7-9.2v-6Z"/><path d="m8.8 12.2 2.2 2.2 4.2-4.4"/></svg>
    title: 크래시와 정전에도 안전하게
    details: 커밋한 페이지는 제자리에서 덮어쓰지 않고, 커밋은 바이트 하나를 바꾸는 순간 반영됩니다. 쓰는 도중 프로세스가 죽어도 커밋은 하나도 잃지 않고, 전원이 나가도 디스크를 기다린 커밋은 온전히 남습니다.
    link: /ko/guide/transactions
    linkText: 커밋이 보장하는 것
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="2.5" y="3.5" width="8" height="5.5" rx="1.3"/><rect x="2.5" y="15" width="8" height="5.5" rx="1.3"/><path d="M15 3.5h3.5l3 3V19a1.5 1.5 0 0 1-1.5 1.5h-5A1.5 1.5 0 0 1 13.5 19V5A1.5 1.5 0 0 1 15 3.5Z"/><path d="M10.5 6.2h3M10.5 17.8h3"/></svg>
    title: 여러 프로세스, 파일 하나
    details: 프로세스끼리는 공유 메모리 없이 운영체제의 파일 잠금만으로 파일을 나눠 씁니다. 쓰기는 한 번에 하나씩 하고, 읽기는 쓰기를 기다리지 않으며, 프로세스가 죽어도 잠금이 남지 않습니다.
    link: /ko/guide/processes
    linkText: 프로세스가 파일을 나눠 쓰는 법
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m15.5 15.5 5 5"/><path d="M7.5 8.5h6M7.5 11.5h4"/></svg>
    title: 코드로도, 문자열로도 쿼리
    details: 각 언어의 빌더나 쿼리 언어로 객체를 거르고 정렬하고 나눠 가져옵니다. 인덱스와 링크, 내장 객체를 따라가며, 두 형태 모두 엔진 안에서 같은 쿼리가 됩니다.
    link: /ko/guide/query-language
    linkText: 쿼리 언어 보기
  - icon: <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3 8.5 4.5L12 12 3.5 7.5Z"/><path d="m3.5 12 8.5 4.5 8.5-4.5"/><path d="m3.5 16.5 8.5 4.5 8.5-4.5"/></svg>
    title: 버전을 따라 바뀌는 스키마
    details: 스키마에 버전을 붙이면, 예전 파일을 열 때 새 컬렉션과 필드, 인덱스를 엔진이 추가하고 그 사이 버전마다 마이그레이션 함수를 실행합니다. 이 모든 일이 트랜잭션 하나로 끝납니다.
    link: /ko/guide/migrations
    linkText: 스키마 마이그레이션
---

## 파일 하나, 모든 언어

네 패키지에서 같은 쿼리를 쓴 모습입니다. 네 패키지 모두 같은 엔진을 감싸므로 한 패키지가 쓴 파일을 다른 패키지가 열어도 같은 객체가 나오고 같은 답이 나옵니다.

<div class="home-parity">
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="rust" />Rust</p>

```rust
let read = db.begin_read()?;
let query = Query::new().filter(Filter::ge("age", 18));
let adults = read.collection("users")?.query(&query)?;
```

</div>
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="node" />Node.js</p>

```ts
const adults = db.read((txn) => {
  const users = txn.collection('users');

  return users.find((q) => q.where('age', '>=', 18));
});
```

</div>
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="dart" />Dart</p>

```dart
final adults = db.read(
  (txn) => txn
      .collection(userSchema)
      .find((q) => q.where(q.age.atLeast(18))),
);
```

</div>
<div class="home-parity-item">
<p class="home-parity-label"><LanguageIcon id="python" />Python</p>

```python
with db.read() as txn:
    adults = txn.collection(User).find(F.age >= 18)
```

</div>
</div>

<p class="home-note">문자열로는 어느 언어에서나 똑같이 씁니다: <code>age &gt;= 18</code></p>

## 객체로 다루는 데이터

컬렉션은 타입이 정해진 필드를 가진 객체를 담고, 쿼리는 그중 일부를 순서대로 찾습니다. 쿼리를 하나 골라 보세요. 아래 객체 가운데 무엇이 어떤 순서로 나오는지, 각 언어로는 어떻게 쓰는지 보여 줍니다.

<QueryDemo />

## 이렇게 씁니다

언어를 고르면 아래 예제가 그 언어로 바뀌고, 이후에 여는 가이드와 API 페이지도 모두 그 언어를 따릅니다.

<LangTabs />

### 스키마를 선언하고 객체 쓰기

::: lang rust

```rust
use darudb::{Collection, Object, OpenOptions, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1).collection(
        Collection::new("users")
            .field("name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::Int, 0)
            .unique("email")
            .index("age"),
    );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;
    users.insert(Object::new().with("name", "Ada").with("age", 36))?;
    users.insert(Object::new().with("name", "Ben").with("age", 24))?;
    txn.commit()?;

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
    email: t.string().optional().unique(),
    age: t.int().default(0).index()
  })
});

const db = Database.open('app.darudb', { schema: app });

db.write((txn) => {
  const users = txn.collection('users');

  users.insert({ name: 'Ada', age: 36 });
  users.insert({ name: 'Ben', age: 24 });
});
```

:::

::: lang dart

```dart
import 'package:darudb/darudb.dart';

part 'main.g.dart';

@Collection('users')
class User {
  const User({this.id, required this.name, this.email, this.age = 0});

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
}

void main() {
  final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

  db.write((txn) {
    final users = txn.collection(userSchema);

    users.insert(const User(name: 'Ada', age: 36));
    users.insert(const User(name: 'Ben', age: 24));
  });
}
```

:::

::: lang python

```python
import darudb
from darudb import field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))

with db.write() as txn:
    users = txn.collection(User)

    users.insert(User(name="Ada", age=36))
    users.insert(User(name="Ben", age=24))
```

:::

### 코드나 문자열로 객체 찾기

::: lang rust

```rust
use darudb::{Database, Filter, Query};

fn find(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    let built = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);
    let written = Query::parse(
        r#"age >= $0 AND name STARTSWITH "A" SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;

    assert_eq!(users.query(&built)?, users.query(&written)?);
    println!("{} adults", users.count(&Query::new().filter(Filter::ge("age", 18)))?);
    Ok(())
}
```

:::

::: lang node

```ts
db.read((txn) => {
  const users = txn.collection('users');

  const built = users.find((q) =>
    q.where('age', '>=', 18).where('name', 'startsWith', 'A').sortBy('age', 'desc').limit(10)
  );
  const written = users.find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [
    18,
    'A'
  ]);

  console.log(
    built,
    written,
    users.count((q) => q.where('age', '>=', 18))
  );
});
```

:::

::: lang dart

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  final built = users.find(
    (q) => q
        .where(q.age.atLeast(18) & q.name.startsWith('A'))
        .sortBy(q.age, descending: true)
        .limit(10),
  );
  final written = users.findText(
    r'age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10',
    [18, 'A'],
  );

  print('$built $written ${users.count((q) => q.where(q.age.atLeast(18)))}');
});
```

:::

::: lang python

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    built = users.find(
        where((F.age >= 18) & F.name.startswith("A")).sort_by(F.age, descending=True).limit(10)
    )
    written = users.find("age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10", 18, "A")

    print(built, written, users.count(F.age >= 18))
```

:::

### 파일을 암호화하고 건강하게 지키기

::: lang rust

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    assert!(db.check()?.is_ok());
    db.backup("backups/secret.darudb")?;
    db.close()
}
```

:::

::: lang node

```ts
const db = await Database.openAsync('secret.darudb', {
  schema: app,
  password: 'correct horse battery staple'
});

if ((await db.checkAsync()).ok) {
  await db.backupAsync('backups/secret.darudb');
}
await db.closeAsync();
```

:::

::: lang dart

```dart
final db = await Database.openAsync(
  'secret.darudb',
  schema: const Schema(1, [userSchema]),
  password: 'correct horse battery staple',
);

if ((await db.checkAsync()).ok) {
  await db.backupAsync('backups/secret.darudb');
}
await db.closeAsync();
```

:::

::: lang python

```python
db = await darudb.Database.open_async(
    "secret.darudb",
    schema=darudb.Schema(1, [User]),
    password="correct horse battery staple",
)

if (await db.check_async()).ok:
    await db.backup_async("backups/secret.darudb")
await db.close_async()
```

:::

## 들어 있는 것

Rust 크레이트에 비동기 API가 따로 없다는 점만 빼면, 모든 패키지가 아래 기능을 다 갖췄습니다. 카드를 누르면 해당 가이드가 열립니다.

<GuideGrid :items="[
  { title: '컬렉션과 객체', desc: '타입이 있는 필드, 기본 키나 엔진이 매기는 id, 고유 인덱스와 일반 인덱스, 링크와 목록, 내장 객체를 다룹니다.', link: '/ko/guide/objects' },
  { title: '쿼리', desc: '비교와 범위, IN, 문자열 일치, null 검사를 정렬하고 나눠 가져오며, 코드로 만들거나 문자열로 씁니다.', link: '/ko/guide/queries' },
  { title: '쿼리 언어', desc: '문자열 쿼리의 연산자와 키워드, 값을 모두 예제와 함께 모았습니다.', link: '/ko/guide/query-language' },
  { title: '트랜잭션', desc: '읽기용 스냅샷, 그리고 디스크를 기다리는 동기 커밋과 기다리지 않는 지연 커밋을 다룹니다.', link: '/ko/guide/transactions' },
  { title: '마이그레이션', desc: '스키마 버전과 이름 바꾸기, 버전 단계마다 실행할 함수를 트랜잭션 하나로 처리합니다.', link: '/ko/guide/migrations' },
  { title: '비동기 API', desc: 'Node.js와 Dart, Python에서는 파일을 쓰는 모든 호출에 Promise와 Future, asyncio 짝이 있습니다.', link: '/ko/guide/async' },
  { title: '암호화', desc: '모든 페이지의 인증 암호화, 비밀번호에서 만드는 키, 그리고 키와 비밀번호 바꾸기를 다룹니다.', link: '/ko/guide/encryption' },
  { title: '여러 프로세스', desc: '파일 잠금만으로 여러 프로세스에 걸쳐 쓰기 하나와 읽기 여럿을 함께 돌립니다.', link: '/ko/guide/processes' },
  { title: '도구', desc: '무결성 검사와 온라인 백업, 압축, 손상된 파일 되살리기를 제공합니다.', link: '/ko/guide/tools' },
  { title: '오류', desc: '모든 실패에 바뀌지 않는 코드가 있고, 그 코드는 모든 언어에서 같습니다.', link: '/ko/guide/errors' }
]" />

## 내 언어로 시작하기

쓰는 언어의 패키지를 추가하세요. 시작하기 페이지가 그 언어의 명령과 코드로 열립니다.

<StartCards :cards="[
  { id: 'rust', note: 'Rust 1.85 이상. derive 기능을 켜면 구조체를 그대로 읽고 씁니다.', install: 'cargo add darudb --features derive', link: '/ko/guide/getting-started' },
  { id: 'node', note: 'Node.js 20 이상. TypeScript 타입이 함께 오고, 지원하는 모든 플랫폼용 엔진이 미리 빌드돼 있습니다.', install: 'npm install darudb', link: '/ko/guide/getting-started' },
  { id: 'dart', note: 'Dart 3.10 또는 Flutter 3.38.1 이상. 빌드 훅이 대상마다 미리 빌드한 엔진을 내려받습니다.', install: 'dart pub add darudb dev:darudb_generator dev:build_runner', link: '/ko/guide/getting-started' },
  { id: 'python', note: 'CPython 3.11 이상, 프리 스레드 3.14 포함. Linux와 macOS, Windows용 휠이 있습니다.', install: 'pip install darudb', link: '/ko/guide/getting-started' }
]" />

<div class="home-cta">

[소개](/ko/guide/introduction) [시작하기](/ko/guide/getting-started) [API](/ko/api/) [비교](/ko/compare)

</div>
