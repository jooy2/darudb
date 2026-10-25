---
title: sled
order: 6
languages: [rust]
---

# sled에서 옮기기

sled 데이터베이스의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 트리와 키, 값이 컬렉션과 객체, 필드에 어떻게 대응하는지, 모든 항목을 복사하는 프로그램, 그리고 애플리케이션에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| sled | DaruDB |
| --- | --- |
| 데이터베이스. 파일 여러 개가 든 폴더 | 데이터베이스. 파일 하나 |
| 값이 레코드인 트리 | 컬렉션 |
| 애플리케이션이 인덱스로 관리하던 트리. 필드 값에서 키로 | 선언한 인덱스나 고유 인덱스. 객체가 바뀌면 엔진이 함께 고칩니다 |
| 키. 바이트 | 기본 키. 타입은 정수, 문자열, 바이트 중 하나 |
| 값. 바이트 | 객체. 애플리케이션이 쓴 방식대로 바이트를 풀어서 만듭니다 |
| 여러 트리에 걸친 트랜잭션 | 쓰기 트랜잭션. 어느 컬렉션에든 걸칩니다 |
| `flush` | 동기 커밋, 또는 지연 커밋 뒤의 `sync` |
| 두 `flush` 사이의 쓰기 | 지연 커밋 |
| 병합 연산자 | 대응하는 것 없음. 트랜잭션 하나 안에서 객체를 읽고 바꿔 다시 씁니다 |
| `watch_prefix`, 곧 구독자 | 대응하는 것 없음. 쓰기가 끝나면 다시 읽습니다 |

- **스키마는 애플리케이션에서 나옵니다.** sled는 바이트를 저장할 뿐 그 모양을 모르므로, 스키마는 애플리케이션이 트리마다 쓰던 내용을 보고 선언합니다. 엔진은 값마다 필드의 타입을 검사하고, 맞지 않는 값이 처음 나오면 복사는 `INVALID_ARGUMENT`로 멈춥니다.
- **키를 그대로 옮깁니다.** 숫자 순서로 정렬되게 빅엔디언으로 저장한 정수 키는 정수 키가 되고, UTF-8 텍스트 키는 문자열 키가 됩니다. 다른 키는 바이트로 두어도 됩니다. 애플리케이션이 여러 부분을 이어 만든 키는 부분마다 필드와 인덱스를 두거나, 문자열 키로 옮깁니다.
- **인덱스는 복사하지 않고 선언합니다.** 이메일에서 사용자의 키를 찾는 트리는 애플리케이션이 직접 관리하던 인덱스입니다. 이것은 복사하지 말고 `email`에 고유 인덱스를 선언하세요. 객체가 들어가면서 엔진이 인덱스를 채웁니다.

## 스키마 선언하기

예제로 옮길 데이터베이스에는 애플리케이션이 쓴 트리가 셋 있습니다.

| 트리 | 키 | 값 |
| --- | --- | --- |
| `users` | 사용자의 id. 빅엔디언 `u64` | JSON: `name`, `null`일 수도 있는 `email`, `age` |
| `posts` | 글의 슬러그. UTF-8 | JSON: 사용자의 키인 `author_id`, `body`, `published_at`, `tags` 목록 |
| `users_by_email` | 이메일. UTF-8 | 그 이메일을 쓰는 사용자의 키 |

`users`는 정수 키를 자동 `id`로 그대로 쓰고, `author_id`는 링크가 되며, `users_by_email`은 고유 인덱스가 됩니다.

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

## 항목 복사하기

프로그램은 애플리케이션이 쓰던 sled 릴리스로 데이터베이스를 읽고, 값마다 serde로 구조체에 풉니다.

```toml
[dependencies]
darudb = "1.3"
sled = "0.34"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

```rust
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
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
    let sled = sled::open("app.sled")?;
    let users = sled.open_tree("users")?;
    let posts = sled.open_tree("posts")?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    copy(
        &db,
        "users",
        users.iter().map(|entry| -> Result<Object, Box<dyn Error>> {
            let (key, value) = entry?;
            let id = u64::from_be_bytes(key.as_ref().try_into()?);
            let user: User = serde_json::from_slice(&value)?;

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
        posts.iter().map(|entry| -> Result<Object, Box<dyn Error>> {
            let (key, value) = entry?;
            let post: Post = serde_json::from_slice(&value)?;

            Ok(Object::new()
                .with("slug", std::str::from_utf8(&key)?)
                .with("author", post.author_id.map(i64::try_from).transpose()?)
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

sled에는 읽기 전용 모드가 없고, 데이터베이스는 한 번에 한 프로세스만 열 수 있습니다. 복사하기 전에 애플리케이션을 닫고, 원본이 바뀌지 않게 폴더를 복사한 것에서 옮기세요. DaruDB의 정수 키는 부호가 있으므로 `i64::MAX`보다 큰 `u64`가 나오면 `try_from`에서 복사가 멈춥니다. 이런 키는 원래 키를 그대로 담는 `Type::Bytes` 키로 두면 됩니다. bincode나 다른 serde 형식으로 쓴 값은 `serde_json` 자리에 그 크레이트를 넣어 읽습니다.

## 복사본 확인하기

트리마다 `users.len()`을 컬렉션의 `len`과 견주고, 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

애플리케이션이 스캔과 직접 관리하던 인덱스 트리로 하던 일은 쿼리가 됩니다. 맞는 인덱스가 있으면 엔진이 그 인덱스로 쿼리를 돌립니다.

| 애플리케이션이 하던 일 | DaruDB 쿼리 언어 |
| --- | --- |
| `users_by_email`에서 키를 얻고, `users`에서 사용자를 읽음 | `users`에서 `email == $0` |
| `users`를 스캔하며 프로그램에서 나이를 검사함 | `age BETWEEN 18 AND 30` |
| 키 접두사로 `users`를 스캔함 | 키의 그 부분을 담은 필드에 `STARTSWITH` |
| `users`를 스캔해 나이가 많은 열 명을 남김 | `SORT BY age DESC LIMIT 10` |
| 어떤 태그가 있는 글을 찾으려고 `posts`를 스캔함 | `posts`에서 `tags CONTAINS "news"` |
| 글을 읽고, 키로 작성자를 읽음 | 작성자 이름으로 글을 찾을 때는 `posts`에서 `author.name == "Alice"` |

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **인덱스를 직접 관리하지 않습니다.** 객체를 쓰면 같은 트랜잭션 안에서 그 컬렉션에 선언한 인덱스가 모두 고쳐지므로, `users_by_email`을 맞추던 코드는 없어집니다.
- **인코딩하지 않습니다.** 객체는 `Object`나 `#[derive(Object)]`를 붙인 구조체로 들어가고 나오며, 구조체는 레코드에서 바로 읽습니다. [컬렉션과 객체](../guide/objects.md)를 보세요.
- **내구성은 커밋마다 고릅니다.** 기본값인 동기 커밋은 반환될 때 디스크에 기록되어 있고, 지연 커밋은 다음 동기 커밋이나 `sync`, `close`에서, 또는 기다린 시간이나 쓴 페이지가 한도를 넘을 때 기록됩니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **여러 프로세스**가 한 파일을 함께 쓸 수 있습니다. 쓰는 쪽은 한 번에 하나, 읽는 쪽은 몇이든 됩니다. [여러 프로세스](../guide/processes.md)를 보세요.
- **구독자가 없습니다.** 데이터가 바뀌어도 애플리케이션을 불러 주는 것이 없습니다. 쓰기가 끝나면 애플리케이션이 보여 주는 것을 다시 읽으세요.
- **파일 전체 암호화**가 엔진에 들어 있습니다. [암호화](../guide/encryption.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
