---
title: SQLite
order: 2
---

# SQLite에서 옮기기

SQLite 데이터베이스의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 테이블과 열, 인덱스가 컬렉션과 필드에 어떻게 대응하는지, 모든 행을 복사하는 스크립트, 그리고 애플리케이션의 쿼리가 어떻게 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| SQLite | DaruDB |
| --- | --- |
| 테이블 | 컬렉션 |
| 행 | 객체 |
| `INTEGER PRIMARY KEY`, 곧 rowid | 자동 `id`, 또는 선언한 정수 키 |
| 다른 열에 건 `PRIMARY KEY` | 그 필드를 기본 키로. 타입은 정수, 문자열, 바이트 중 하나 |
| 여러 열로 된 기본 키 | 대응하는 것 없음. 자동 `id`를 쓰거나, 여러 값을 이어 붙인 문자열 키 하나로 |
| `INTEGER` | 64비트 정수 필드 |
| `REAL` | 64비트 실수 필드 |
| `TEXT` | 문자열 필드 |
| `BLOB` | 바이트 필드 |
| `NOT NULL` | 필수 필드. `NULL`을 허용하는 열은 선택 필드 |
| `DEFAULT` | 필드의 기본값 |
| `UNIQUE`, 또는 열 하나에 건 고유 인덱스 | 고유 인덱스 |
| 열 하나에 건 인덱스 | 인덱스 |
| 여러 열에 건 인덱스 | 아직 대응하는 것 없음. 쿼리를 가장 많이 좁히는 열에 인덱스를 둡니다 |
| 외래 키 | 링크. 다른 객체의 기본 키를 담습니다 |
| 두 테이블을 잇는 연결 테이블 | 한쪽에 둔 링크 목록, 또는 값 목록 |
| 뷰, 트리거, `CHECK` 제약 | 대응하는 것 없음. 그 규칙은 애플리케이션에 둡니다 |

- **타입을 검사합니다.** SQLite 열은 선언한 타입과 관계없이 어떤 타입의 값이든 담을 수 있지만, DaruDB 필드는 자기 타입의 값만 담습니다. 복사하기 전에 `SELECT count(*) FROM users WHERE typeof(age) != 'integer'`처럼 타입이 다른 값을 찾아 스크립트에서 바꾸세요. 그러지 않으면 첫 값에서 `INVALID_ARGUMENT`로 멈춥니다.
- **불리언**을 `0`과 `1`로 저장했다면, 스크립트에서 값마다 바꿔 불리언 필드로 옮길 수 있습니다.
- **날짜와 시각**은 두 데이터베이스 모두 따로 타입이 없습니다. UTC 기준 ISO 8601 문자열은 바이트 순서가 곧 시간 순서이므로, 문자열 필드로 두어도 비교하고 정렬할 수 있습니다. 초나 밀리초 단위 숫자는 정수로 둡니다.
- **키를 그대로 옮깁니다.** 행의 키를 객체의 키로 옮기면 외래 키가 모두 링크로 살아남습니다. 자동 `id`를 쓰는 컬렉션은 옮겨 넣은 가장 큰 `id` 다음부터 번호를 이어서 매깁니다.

## 스키마 선언하기

예제로 옮길 데이터베이스는 사용자와 그 글, 그리고 연결 테이블에 담긴 글마다의 태그입니다.

```sql
CREATE TABLE users (
  id    INTEGER PRIMARY KEY,
  name  TEXT NOT NULL,
  email TEXT UNIQUE,
  age   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX users_age ON users (age);

CREATE TABLE posts (
  slug         TEXT PRIMARY KEY,
  author_id    INTEGER REFERENCES users (id),
  body         TEXT NOT NULL,
  published_at TEXT
);
CREATE INDEX posts_author ON posts (author_id);

CREATE TABLE post_tags (
  post_slug TEXT NOT NULL REFERENCES posts (slug),
  tag       TEXT NOT NULL,
  PRIMARY KEY (post_slug, tag)
);
```

`users`의 정수 키는 자동 `id`로 그대로 두고, `author_id`는 링크로, 연결 테이블은 글마다의 태그 목록으로 바꿉니다. 태그 목록에는 인덱스를 두어, 태그로 찾는 쿼리가 그 태그가 있는 글만 읽게 합니다.

::: lang rust

```rust
use darudb::{Collection, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email")
                .index("age"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .optional("author", Type::link("users"))
                .field("body", Type::String)
                .optional("published_at", Type::String)
                .field("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}
```

:::

::: lang node

```ts
import { collection, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index()
  }),
  posts: collection({
    slug: t.string().primaryKey(),
    author: t.link('users').optional().index(),
    body: t.string(),
    publishedAt: t.string().optional(),
    tags: t.list(t.string()).index()
  })
});
```

:::

::: lang dart

```dart
import 'package:darudb/darudb.dart';

part 'schema.g.dart';

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

@Collection('posts')
class Post {
  const Post({
    required this.slug,
    this.author,
    required this.body,
    this.publishedAt,
    required this.tags,
  });

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
  final String body;
  @Name('published_at')
  final String? publishedAt;
  @Index()
  final List<String> tags;
}

const app = Schema(1, [userSchema, postSchema]);
```

:::

## 행 복사하기

::: lang rust

스크립트는 `rusqlite` 크레이트로 원래 파일을 읽습니다. `bundled` 기능을 켜면 SQLite를 프로그램에 함께 컴파일합니다.

```toml
[dependencies]
darudb = "1.0"
rusqlite = { version = "0.40", features = ["bundled"] }
```

```rust
use std::collections::HashMap;
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use rusqlite::{Connection, OpenFlags};

/// `objects`를 컬렉션 `name`에 지연 커밋 한 번으로 넣습니다.
fn insert_all(db: &Database, name: &str, objects: Vec<Object>) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    let mut collection = txn.collection(name)?;

    for object in objects {
        collection.insert(object)?;
    }

    txn.commit_deferred()
}

fn main() -> Result<(), Box<dyn Error>> {
    let sqlite = Connection::open_with_flags("app.sqlite", OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    let mut users = sqlite.prepare("SELECT id, name, email, age FROM users")?;
    let mut rows = users.query([])?;
    let mut batch = Vec::new();

    while let Some(row) = rows.next()? {
        batch.push(
            Object::new()
                .with("id", row.get::<_, i64>(0)?)
                .with("name", row.get::<_, String>(1)?)
                .with("email", row.get::<_, Option<String>>(2)?)
                .with("age", row.get::<_, i64>(3)?),
        );

        if batch.len() == 1000 {
            insert_all(&db, "users", std::mem::take(&mut batch))?;
        }
    }

    insert_all(&db, "users", batch)?;

    // 연결 테이블은 글마다의 목록이 됩니다.
    let mut tags: HashMap<String, Vec<Value>> = HashMap::new();
    let mut post_tags = sqlite.prepare("SELECT post_slug, tag FROM post_tags")?;
    let mut rows = post_tags.query([])?;

    while let Some(row) = rows.next()? {
        tags.entry(row.get(0)?).or_default().push(Value::from(row.get::<_, String>(1)?));
    }

    let mut posts = sqlite.prepare("SELECT slug, author_id, body, published_at FROM posts")?;
    let mut rows = posts.query([])?;
    let mut batch = Vec::new();

    while let Some(row) = rows.next()? {
        let slug: String = row.get(0)?;

        batch.push(
            Object::new()
                .with("tags", tags.remove(&slug).unwrap_or_default())
                .with("slug", slug)
                .with("author", row.get::<_, Option<i64>>(1)?)
                .with("body", row.get::<_, String>(2)?)
                .with("published_at", row.get::<_, Option<String>>(3)?),
        );

        if batch.len() == 1000 {
            insert_all(&db, "posts", std::mem::take(&mut batch))?;
        }
    }

    insert_all(&db, "posts", batch)?;

    // 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
    db.close()?;

    Ok(())
}
```

:::

::: lang node

스크립트는 `node:sqlite`로 원래 파일을 읽습니다. Node.js 22.13부터 내장된 모듈이라 따로 설치할 것이 없습니다.

```js
import { DatabaseSync } from 'node:sqlite';
import { Database } from 'darudb';

const sqlite = new DatabaseSync('app.sqlite', { readOnly: true });
const db = Database.open('app.darudb', { schema: app });

/** `rows`를 컬렉션 `name`에 트랜잭션마다 천 개씩 복사합니다. */
function copy(rows, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for (const row of rows) {
    batch.push(convert(row));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

copy(sqlite.prepare('SELECT id, name, email, age FROM users').iterate(), 'users', (row) => ({
  id: row.id,
  name: row.name,
  email: row.email,
  age: row.age
}));

// 연결 테이블은 글마다의 목록이 됩니다.
const tags = new Map();

for (const { post_slug, tag } of sqlite.prepare('SELECT post_slug, tag FROM post_tags').iterate()) {
  tags.set(post_slug, [...(tags.get(post_slug) ?? []), tag]);
}

copy(
  sqlite.prepare('SELECT slug, author_id, body, published_at FROM posts').iterate(),
  'posts',
  (row) => ({
    slug: row.slug,
    author: row.author_id,
    body: row.body,
    publishedAt: row.published_at,
    tags: tags.get(row.slug) ?? []
  })
);

// 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close();
sqlite.close();
```

`node:sqlite`는 정수를 number로 읽습니다. 값이 2^53을 넘는 열은 문에서 `setReadBigInts(true)`를 부르고, 필드를 `t.bigint()`로 선언하세요.

:::

::: lang dart

스크립트는 `sqlite3` 패키지로 원래 파일을 읽습니다. 이 패키지는 빌드 훅으로 SQLite를 함께 가져옵니다.

```yaml
dependencies:
  darudb: ^1.0.0
  sqlite3: ^3.7.0
```

```dart
import 'package:darudb/darudb.dart';
import 'package:sqlite3/sqlite3.dart';

import 'schema.dart';

void main() {
  final sqlite = sqlite3.open('app.sqlite', mode: OpenMode.readOnly);
  final db = Database.open('app.darudb', schema: app);

  /// [rows]를 컬렉션에 트랜잭션마다 천 개씩 복사합니다.
  void copy<T>(
    Iterable<Row> rows,
    CollectionSchema<T, QueryBuilder<T>, Object> collection,
    T Function(Row row) convert,
  ) {
    final batch = <T>[];

    void flush() {
      db.write(
        (txn) => txn.collection(collection).insertMany(batch),
        durability: Durability.deferred,
      );
      batch.clear();
    }

    for (final row in rows) {
      batch.add(convert(row));

      if (batch.length == 1000) {
        flush();
      }
    }

    if (batch.isNotEmpty) {
      flush();
    }
  }

  copy(
    sqlite.select('SELECT id, name, email, age FROM users'),
    userSchema,
    (row) => User(
      id: row['id'] as int,
      name: row['name'] as String,
      email: row['email'] as String?,
      age: row['age'] as int,
    ),
  );

  // 연결 테이블은 글마다의 목록이 됩니다.
  final tags = <String, List<String>>{};

  for (final row in sqlite.select('SELECT post_slug, tag FROM post_tags')) {
    tags.putIfAbsent(row['post_slug'] as String, () => []).add(row['tag'] as String);
  }

  copy(
    sqlite.select('SELECT slug, author_id, body, published_at FROM posts'),
    postSchema,
    (row) => Post(
      slug: row['slug'] as String,
      author: row['author_id'] == null ? null : Link<User>(row['author_id'] as int),
      body: row['body'] as String,
      publishedAt: row['published_at'] as String?,
      tags: tags[row['slug']] ?? [],
    ),
  );

  // 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
  db.close();
  sqlite.close();
}
```

:::

원래 파일은 읽기 전용으로 열어서 그대로 남습니다. 연결 테이블을 먼저 메모리에 다 읽어 두기 때문에 글마다 태그와 함께 한 번에 쓸 수 있습니다. 연결 테이블이 메모리에 담기에 너무 크면, `post_slug`로 정렬해 읽으면서 `slug`로 정렬한 글과 나란히 맞춰 가세요.

## 복사본 확인하기

테이블과 컬렉션마다 개수를 세어 비교합니다. `SELECT count(*) FROM users`의 결과를 `users` 컬렉션의 개수(<LangCode rust="len" node="count" dart="count" />)와 견주면 됩니다. 그다음 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌리면, 모든 객체를 인덱스와도 맞춰 봅니다.

## 쿼리

쿼리 언어는 `WHERE` 절과 비슷하게 읽힙니다.

| SQL | DaruDB 쿼리 언어 |
| --- | --- |
| `WHERE age >= 18` | `age >= 18` |
| `WHERE email IS NULL` | `email IS NULL` |
| `WHERE age BETWEEN 18 AND 30` | `age BETWEEN 18 AND 30` |
| `WHERE id IN (1, 2)` | `id IN [1, 2]` |
| `WHERE name LIKE 'A%'` | `name STARTSWITH "A"` |
| `WHERE name LIKE '%li%'` | `name CONTAINS "li"` |
| `ORDER BY age DESC LIMIT 10 OFFSET 20` | `SORT BY age DESC LIMIT 10 OFFSET 20` |
| `SELECT count(*) ...` | 같은 쿼리로 `count` |
| `JOIN users ON users.id = posts.author_id WHERE users.name = 'Alice'` | `posts`에서 `author.name == "Alice"` |
| `JOIN post_tags ... WHERE tag = 'news'` | `posts`에서 `tags CONTAINS "news"` |

- `LIKE`는 ASCII 문자의 대소문자를 가리지 않지만, `STARTSWITH`와 `CONTAINS`는 바이트를 그대로 비교하므로 가립니다. 대소문자 없이 찾으려면 필드를 소문자로 바꾼 사본을 따로 두고 그 필드로 찾으세요.
- 조인은 링크를 따라가는 경로가 되고, 경로는 링크가 가리키는 객체를 읽습니다. `GROUP BY`, 개수 말고 다른 집계, 서브쿼리, 링크가 아닌 것으로 하는 조인에는 대응하는 것이 없습니다.
- 프로그램 바깥에서 들어온 값은 prepared statement에서처럼 `$0` 같은 매개변수로 넘기세요. 쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **트랜잭션**은 `BEGIN`과 `COMMIT` 대신 <LangCode rust="begin_read와 begin_write" node="db.read와 db.write" dart="db.read와 db.write" />를 쓰고, 읽기도 트랜잭션 안에서 합니다. 커밋이 무엇을 보장하는지는 [트랜잭션](../guide/transactions.md)에 있습니다.
- **스키마 변경**은 `ALTER TABLE`을 실행하는 대신 스키마 버전을 올립니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가하고, 나머지는 마이그레이션에 적습니다. [마이그레이션](../guide/migrations.md)을 보세요.
- **결과는 평범한 객체**여서 트랜잭션이 끝나도 값이 그대로 남습니다.
