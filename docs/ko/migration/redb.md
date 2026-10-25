---
title: redb
order: 5
languages: [rust]
---

# redb에서 옮기기

redb 파일의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 테이블과 키, 값이 컬렉션과 객체, 필드에 어떻게 대응하는지, 모든 항목을 복사하는 프로그램, 그리고 애플리케이션에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| redb | DaruDB |
| --- | --- |
| 데이터베이스. 파일 하나 | 데이터베이스. 파일 하나 |
| 값이 레코드인 테이블 | 컬렉션 |
| 애플리케이션이 인덱스로 관리하던 테이블. 필드 값에서 키로 | 선언한 인덱스나 고유 인덱스. 객체가 바뀌면 엔진이 함께 고칩니다 |
| 키 하나에 값 여럿을 두는 멀티맵 테이블 | 객체의 목록 필드, 또는 따로 둔 컬렉션 |
| `u64`, `i64`, `&str`, `&[u8]` 키 | 기본 키. 타입은 정수, 문자열, 바이트 중 하나 |
| 튜플 키 | 부분마다 필드와 인덱스, 또는 부분을 이어 만든 문자열 키 |
| 튜플 값, 또는 애플리케이션이 `redb::Value`를 구현한 타입의 값 | 객체. 부분마다 필드를 둡니다 |
| `Option<T>` | 선택 필드 |
| `Durability::Immediate` | 동기 커밋. 기본값입니다 |
| `Durability::None` | 지연 커밋. 다음 동기 커밋이나 `sync`, `close`에서 디스크에 기록됩니다 |
| 세이브포인트 | 대응하는 것 없음. 대신 쓰기 트랜잭션을 중단합니다 |

- **스키마는 테이블 정의에서 나옵니다.** redb 테이블의 키와 값 타입은 파일이 아니라 프로그램에 있으므로, DaruDB 스키마는 애플리케이션이 여는 `TableDefinition`을 보고 선언합니다.
- **키를 그대로 옮깁니다.** DaruDB의 키는 부호 있는 64비트 정수, 문자열, 바이트 중 하나입니다. `u64` 키는 `i64::MAX`를 넘지 않으면 그대로 들어갑니다. 넘는 키는 빅엔디언 8바이트로 두면 정렬 순서도 같습니다.
- **인덱스는 복사하지 않고 선언합니다.** 이메일에서 사용자의 키를 찾는 테이블은 애플리케이션이 직접 관리하던 인덱스입니다. 이것은 복사하지 말고 `email`에 고유 인덱스를 선언하세요. 객체가 들어가면서 엔진이 인덱스를 채웁니다.

## 스키마 선언하기

예제로 옮길 파일에는 애플리케이션이 정의한 테이블이 넷 있습니다.

```rust
use redb::{MultimapTableDefinition, TableDefinition};

// 이름, 이메일, 나이
const USERS: TableDefinition<u64, (&str, Option<&str>, u32)> = TableDefinition::new("users");
// 작성자, 본문, 게시 시각
const POSTS: TableDefinition<&str, (Option<u64>, &str, Option<&str>)> =
    TableDefinition::new("posts");
// 글마다의 태그. 글의 슬러그 아래에 둡니다.
const POST_TAGS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("post_tags");
// 이메일마다 그 이메일을 쓰는 사용자의 키.
const USERS_BY_EMAIL: TableDefinition<&str, u64> = TableDefinition::new("users_by_email");
```

`users`는 정수 키를 자동 `id`로 그대로 쓰고, 작성자는 링크가 됩니다. 멀티맵 테이블은 글마다의 태그 목록이 되고, 태그로 쿼리할 때 그 태그가 있는 글만 읽도록 인덱스를 둡니다. `users_by_email`은 고유 인덱스가 됩니다.

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

프로그램은 redb 파일을 읽기 전용으로 열고 새 DaruDB 파일을 씁니다.

```toml
[dependencies]
darudb = "1.3"
redb = "4.3"
```

```rust
use std::error::Error;

use darudb::{Database, Object, OpenOptions, Value};
use redb::{ReadOnlyDatabase, ReadableDatabase, ReadableTable};

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
    let redb = ReadOnlyDatabase::open("app.redb")?;
    let rtxn = redb.begin_read()?;
    let users = rtxn.open_table(USERS)?;
    let posts = rtxn.open_table(POSTS)?;
    let post_tags = rtxn.open_multimap_table(POST_TAGS)?;
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;

    copy(
        &db,
        "users",
        users.iter()?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (id, user) = entry?;
            let (name, email, age) = user.value();

            Ok(Object::new()
                .with("id", i64::try_from(id.value())?)
                .with("name", name)
                .with("email", email)
                .with("age", age))
        }),
    )?;

    copy(
        &db,
        "posts",
        posts.iter()?.map(|entry| -> Result<Object, Box<dyn Error>> {
            let (slug, post) = entry?;
            let (author, body, published_at) = post.value();
            // 멀티맵은 글의 태그를 바이트 순서로 줍니다.
            let mut tags = Vec::new();

            for tag in post_tags.get(slug.value())? {
                tags.push(Value::from(tag?.value()));
            }

            Ok(Object::new()
                .with("slug", slug.value())
                .with("author", author.map(i64::try_from).transpose()?)
                .with("body", body)
                .with("published_at", published_at)
                .with("tags", tags))
        }),
    )?;

    // 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
    db.close()?;

    Ok(())
}
```

`ReadOnlyDatabase`는 파일에 쓰지 않고 열므로 파일은 그대로 남습니다. redb 파일은 한 번에 한 프로세스만 열 수 있으므로 애플리케이션은 닫아 둡니다. 객체마다 원래 키를 그대로 가지고 들어가므로, 어느 컬렉션을 먼저 복사하든 `author`는 맞는 사용자를 가리킵니다. 애플리케이션이 만든 타입의 값은 애플리케이션이 읽던 대로 그 타입을 `TableDefinition`에 넣어 읽고, 필드마다 옮깁니다.

## 복사본 확인하기

테이블마다 `users.len()?`을 컬렉션의 `len`과 견주고, 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

애플리케이션이 범위 읽기와 직접 관리하던 인덱스 테이블로 하던 일은 쿼리가 됩니다. 맞는 인덱스가 있으면 엔진이 그 인덱스로 쿼리를 돌립니다.

| 애플리케이션이 하던 일 | DaruDB 쿼리 언어 |
| --- | --- |
| `users_by_email`에서 키를 얻고, `users`에서 사용자를 읽음 | `users`에서 `email == $0` |
| 사용자를 모두 훑으며 프로그램에서 나이를 검사함 | `age BETWEEN 18 AND 30` |
| 사용자를 모두 훑어 나이가 많은 열 명을 남김 | `SORT BY age DESC LIMIT 10` |
| `post_tags`에서 글의 태그를 읽음 | 글의 `tags` 필드 |
| 어떤 태그가 있는 글을 찾으려고 `post_tags`를 훑음 | `posts`에서 `tags CONTAINS "news"` |
| 글을 읽고, 키로 작성자를 읽음 | 작성자 이름으로 글을 찾을 때는 `posts`에서 `author.name == "Alice"` |

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **인덱스를 직접 관리하지 않습니다.** 객체를 쓰면 같은 트랜잭션 안에서 그 컬렉션에 선언한 인덱스가 모두 고쳐지므로, `users_by_email`과 `post_tags`를 맞추던 코드는 없어집니다.
- **튜플이 아니라 객체입니다.** 객체는 `Object`나 `#[derive(Object)]`를 붙인 구조체로 들어가고 나오며, 구조체는 레코드에서 바로 읽습니다. [컬렉션과 객체](../guide/objects.md)를 보세요.
- **여러 프로세스**가 한 파일을 함께 쓸 수 있습니다. 쓰는 쪽은 한 번에 하나, 읽는 쪽은 몇이든 됩니다. [여러 프로세스](../guide/processes.md)를 보세요.
- **트랜잭션**의 모양은 같습니다. 읽기는 끝날 때까지 스냅샷 하나를 보고, 쓰기는 통째로 커밋되거나 통째로 중단됩니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **파일 전체 암호화**가 엔진에 들어 있습니다. [암호화](../guide/encryption.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
