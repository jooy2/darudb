---
title: LMDB
order: 4
languages: [rust, node, python]
---

# LMDB에서 옮기기

LMDB 환경의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 이름 있는 데이터베이스와 키, 값이 컬렉션과 객체, 필드에 어떻게 대응하는지, 모든 항목을 복사하는 프로그램(Rust는 heed, Node.js는 lmdb-js, Python은 `lmdb` 모듈), 그리고 애플리케이션에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| LMDB | DaruDB |
| --- | --- |
| 환경. `data.mdb`와 `lock.mdb`가 든 폴더 | 데이터베이스. 파일 하나 |
| 값이 레코드인 이름 있는 데이터베이스 | 컬렉션 |
| 애플리케이션이 인덱스로 관리하던 이름 있는 데이터베이스. 필드 값에서 키로 | 선언한 인덱스나 고유 인덱스. 객체가 바뀌면 엔진이 함께 고칩니다 |
| 키 | 기본 키. 타입은 정수, 문자열, 바이트 중 하나 |
| 값 | 객체. 애플리케이션이 쓴 방식대로 바이트를 풀어서 만듭니다 |
| 키 하나에 값 여럿을 두는 `dupsort` 데이터베이스 | 객체의 목록 필드, 또는 따로 둔 컬렉션 |
| 읽기 트랜잭션, 쓰기 트랜잭션 | 읽기 트랜잭션, 쓰기 트랜잭션 |
| `NOSYNC`나 `NO_SYNC`를 준 커밋 | 지연 커밋 |
| 맵 크기 | 정할 것이 없습니다. 파일은 필요한 만큼 커집니다 |

- **스키마는 애플리케이션에서 나옵니다.** LMDB는 바이트를 저장할 뿐 그 모양을 모르므로, 스키마는 애플리케이션이 이름 있는 데이터베이스마다 쓰던 내용을 보고 선언합니다. 엔진은 값마다 필드의 타입을 검사하고, 맞지 않는 값이 처음 나오면 복사는 `INVALID_ARGUMENT`로 멈춥니다. 값은 애플리케이션이 하던 대로 풀고, 타입이 다른 값은 프로그램에서 바꾸세요.
- **키를 그대로 옮깁니다.** 숫자 순서로 정렬되게 빅엔디언으로 저장한 정수 키는 정수 키가 되고, 문자열 키는 문자열 키로 남습니다. 다른 키는 바이트로 두어도 됩니다. `user/42/post/7`처럼 애플리케이션이 여러 부분을 이어 만든 키는 부분마다 필드와 인덱스를 두거나, 문자열 키로 옮깁니다.
- **인덱스는 복사하지 않고 선언합니다.** 이메일에서 사용자의 키를 찾는 이름 있는 데이터베이스는 애플리케이션이 직접 관리하던 인덱스입니다. 이것은 복사하지 말고 `email`에 고유 인덱스를 선언하세요. 객체가 들어가면서 엔진이 인덱스를 채우고, 그 뒤로도 맞게 유지합니다.

## 스키마 선언하기

예제로 옮길 환경에는 애플리케이션이 쓴 이름 있는 데이터베이스가 셋 있습니다.

| 이름 있는 데이터베이스 | 키 | 값 |
| --- | --- | --- |
| `users` | 사용자의 id. 부호 없는 64비트 정수, 빅엔디언 | `name`, 없을 수도 있는 `email`, `age` |
| `posts` | 글의 슬러그. 문자열 | 사용자의 키인 `author_id`, `body`, `published_at`, `tags` 목록. Node.js에서는 `authorId`, `publishedAt`이라는 이름입니다 |
| `users_by_email` | 이메일 | 그 이메일을 쓰는 사용자의 키 |

::: lang rust

값은 heed의 `SerdeJson`으로 쓴 JSON입니다. DaruDB에서 `users`는 정수 키를 자동 `id`로 그대로 쓰고, `author_id`는 링크가 되며, `users_by_email`은 고유 인덱스가 됩니다.

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

값은 lmdb-js의 기본 인코딩인 MessagePack이고, 키는 숫자가 숫자 순서로 정렬되는 기본 인코딩입니다. DaruDB에서 `users`는 정수 키를 자동 `id`로 그대로 쓰고, `authorId`는 링크가 되며, `users_by_email`은 고유 인덱스가 됩니다.

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

::: lang python

값은 JSON이고, `users`의 키는 `struct.pack(">Q", id)`로 만든 것입니다. DaruDB에서 `users`는 정수 키를 자동 `id`로 그대로 쓰고, `author_id`는 링크가 되며, `users_by_email`은 고유 인덱스가 됩니다.

```python
import darudb
from darudb import field


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)


@darudb.collection("posts")
class Post:
    slug: str = field(primary_key=True)
    author: int | None = field(default=None, link=User, index=True)
    body: str
    published_at: str | None = None
    tags: list[str] = field(index=True)


app = darudb.Schema(1, [User, Post])
```

:::

## 항목 복사하기

::: lang rust

프로그램은 애플리케이션이 쓰던 크레이트인 heed로 환경을 읽고, 값마다 serde로 구조체에 풉니다.

```toml
[dependencies]
darudb = "1.3"
heed = "0.22"
serde = { version = "1", features = ["derive"] }
```

```rust
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use heed::byteorder::BigEndian;
use heed::types::{SerdeJson, Str, U64};
use heed::{EnvFlags, EnvOpenOptions};
use serde::Deserialize;

/// 애플리케이션이 저장한 그대로의 사용자.
#[derive(Deserialize)]
struct User {
    name: String,
    email: Option<String>,
    age: i64,
}

/// 애플리케이션이 저장한 그대로의 글.
#[derive(Deserialize)]
struct Post {
    author_id: Option<u64>,
    body: String,
    published_at: Option<String>,
    tags: Vec<String>,
}

/// `objects`를 지연 커밋마다 천 개씩 `name` 컬렉션에 복사합니다.
fn copy<I>(db: &Database, name: &str, objects: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = Result<Object, Box<dyn Error>>>,
{
    let mut objects = objects.into_iter().peekable();

    while objects.peek().is_some() {
        let mut txn = db.begin_write()?;
        let mut collection = txn.collection(name)?;

        for object in objects.by_ref().take(1000) {
            collection.insert(object?)?;
        }

        txn.commit_deferred()?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    // SAFETY: LMDB는 파일을 메모리에 매핑하고, 한 프로세스에서 환경을 두 번 열면
    // 잠금이 깨집니다. 이 프로그램은 읽기 전용으로 한 번만 엽니다.
    let env = unsafe {
        EnvOpenOptions::new()
            .max_dbs(8)
            .flags(EnvFlags::READ_ONLY)
            .open("app.lmdb")?
    };
    let rtxn = env.read_txn()?;
    let users: heed::Database<U64<BigEndian>, SerdeJson<User>> =
        env.open_database(&rtxn, Some("users"))?.ok_or("no users database")?;
    let posts: heed::Database<Str, SerdeJson<Post>> =
        env.open_database(&rtxn, Some("posts"))?.ok_or("no posts database")?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    copy(
        &db,
        "users",
        users.iter(&rtxn)?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (id, user) = entry?;

            Ok(Object::new()
                .with("id", i64::try_from(id)?)
                .with("name", user.name)
                .with("email", user.email)
                .with("age", user.age))
        }),
    )?;

    copy(
        &db,
        "posts",
        posts.iter(&rtxn)?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (slug, post) = entry?;
            let author = post.author_id.map(i64::try_from).transpose()?;

            Ok(Object::new()
                .with("slug", slug)
                .with("author", author)
                .with("body", post.body)
                .with("published_at", post.published_at)
                .with("tags", post.tags.into_iter().map(Value::from).collect::<Vec<_>>()))
        }),
    )?;

    // 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
    db.close()?;

    Ok(())
}
```

DaruDB의 정수 키는 부호가 있으므로 `i64::MAX`보다 큰 `u64`는 정수 키가 될 수 없습니다. 그런 키가 나오면 `try_from`에서 복사가 멈춥니다. 이런 키는 `id.to_be_bytes()`를 담는 `Type::Bytes` 키로 두면 됩니다. 다른 serde 형식으로 쓴 값도 `SerdeJson` 자리에 heed의 `SerdeBincode`나 `SerdeRmp`를 넣어 같은 방법으로 읽습니다.

:::

::: lang node

스크립트는 애플리케이션이 쓰던 패키지인 lmdb-js로 환경을 읽습니다.

```js
import { open } from 'lmdb';
import { Database } from 'darudb';

const lmdb = open({ path: 'app.lmdb', readOnly: true });
const users = lmdb.openDB('users');
const posts = lmdb.openDB('posts');
const db = Database.open('app.darudb', { schema: app });

/** `entries`를 트랜잭션마다 천 개씩 `name` 컬렉션에 복사합니다. */
function copy(entries, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for (const entry of entries) {
    batch.push(convert(entry));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

copy(users.getRange(), 'users', ({ key, value }) => ({
  id: key,
  name: value.name,
  email: value.email ?? null,
  age: value.age
}));

copy(posts.getRange(), 'posts', ({ key, value }) => ({
  slug: key,
  author: value.authorId ?? null,
  body: value.body,
  publishedAt: value.publishedAt ?? null,
  tags: value.tags
}));

// 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close();
await lmdb.close();
```

`openDB`에는 애플리케이션이 데이터베이스마다 주던 `encoding`과 `keyEncoding`을 그대로 줍니다(예: `encoding: 'json'`). `getRange`는 읽기 스냅샷 하나에서 데이터베이스를 키 순서로 훑습니다.

:::

::: lang python

스크립트는 애플리케이션이 쓰던 패키지인 `lmdb` 모듈로 환경을 읽습니다.

```python
import json
import struct

import darudb
import lmdb

from schema import Post, User, app

env = lmdb.open("app.lmdb", readonly=True, max_dbs=8)
users = env.open_db(b"users", create=False)
posts = env.open_db(b"posts", create=False)
db = darudb.Database.open("app.darudb", schema=app)


def copy(entries, cls, convert):
    """``entries``를 트랜잭션마다 천 개씩 ``cls``의 컬렉션에 복사합니다."""
    batch = []

    def flush():
        with db.write(durability="deferred") as txn:
            txn.collection(cls).insert_many(batch)

        batch.clear()

    for entry in entries:
        batch.append(convert(entry))

        if len(batch) == 1000:
            flush()

    if batch:
        flush()


def user(key, value):
    fields = json.loads(value)

    return User(id=struct.unpack(">Q", key)[0], name=fields["name"], email=fields["email"], age=fields["age"])


def post(key, value):
    fields = json.loads(value)

    return Post(
        slug=key.decode(),
        author=fields["author_id"],
        body=fields["body"],
        published_at=fields["published_at"],
        tags=fields["tags"],
    )


with env.begin() as txn:
    copy(txn.cursor(db=users), User, lambda entry: user(*entry))
    copy(txn.cursor(db=posts), Post, lambda entry: post(*entry))

# 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close()
env.close()
```

커서는 트랜잭션의 스냅샷에서 이름 있는 데이터베이스를 키 순서로 훑으며, 키와 값을 `bytes`로 줍니다. 애플리케이션이 pickle이나 MessagePack으로 쓴 값은 `json.loads` 자리에 `pickle.loads`나 `msgpack.unpackb`를 넣어 읽습니다.

:::

환경은 읽기 전용으로 열므로 그대로 남습니다. 객체마다 원래 키를 그대로 가지고 들어가므로, 어느 컬렉션을 먼저 복사하든 `author`는 맞는 사용자를 가리킵니다.

## 복사본 확인하기

이름 있는 데이터베이스와 컬렉션의 개수를 세어 견줍니다. <LangCode rust="users.len(&rtxn)" node="users.getCount()" python="txn.stat(users)['entries']" />를 `users` 컬렉션의 <LangCode rust="len" node="count" python="count" />와 비교하세요. 그다음 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

애플리케이션이 커서와 직접 관리하던 인덱스 데이터베이스로 하던 일은 쿼리가 됩니다. 맞는 인덱스가 있으면 엔진이 그 인덱스로 쿼리를 돌립니다.

| 애플리케이션이 하던 일 | DaruDB 쿼리 언어 |
| --- | --- |
| `users_by_email`에서 이메일을 찾고, `users`에서 그 키를 찾음 | `users`에서 `email == $0` |
| 나이 인덱스 데이터베이스를 18부터 30까지 훑음 | `age BETWEEN 18 AND 30` |
| 사용자를 모두 훑으며 프로그램에서 필드를 검사함 | 그 검사를 쿼리로: `name STARTSWITH "A"` |
| 나이 인덱스 데이터베이스를 거꾸로 훑어 열 개를 남김 | `SORT BY age DESC LIMIT 10` |
| 태그에서 글로 가는 `dupsort` 데이터베이스를 관리함 | `posts`에서 `tags CONTAINS "news"` |
| 글을 읽고, 키로 작성자를 읽음 | 작성자 이름으로 글을 찾을 때는 `posts`에서 `author.name == "Alice"` |

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **인덱스를 직접 관리하지 않습니다.** 객체를 쓰면 같은 트랜잭션 안에서 그 컬렉션에 선언한 인덱스가 모두 고쳐지므로, `users_by_email`을 맞추던 코드는 없어집니다.
- **인코딩하지 않습니다.** 객체는 <LangCode rust="Object나 #[derive(Object)]를 붙인 구조체" node="스키마의 타입을 따르는 평범한 객체" python="컬렉션 클래스의 인스턴스" />로 들어가고 나오므로, 값을 인코딩하고 풀던 코드도 없어집니다.
- **트랜잭션**의 모양은 같습니다. 스레드와 프로세스를 통틀어 쓰는 쪽은 한 번에 하나이고, 읽는 쪽은 몇이든 됩니다. 읽기는 끝날 때까지 스냅샷 하나를 봅니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **맵 크기가 없습니다.** 파일은 필요한 만큼 커지고, [압축하면](../guide/tools.md#파일-압축하기) 빈 공간을 돌려줍니다.
- **파일 전체 암호화**가 엔진에 들어 있습니다. [암호화](../guide/encryption.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
