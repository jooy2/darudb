---
title: Schema
order: 7
---

# Schema

`Schema`는 데이터베이스의 컬렉션과 그 객체가 담는 내용을 버전 하나와 함께 선언합니다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Schema
```

애플리케이션은 실행 중에 `Schema::new`와 [`Collection`](./collection.md)으로 스키마를 만들고, [`OpenOptions::schema`](./open-options.md#schema)로 이 스키마를 주며 파일을 엽니다. 처음 열 때 스키마를 파일에 저장합니다. 그 뒤로는 열 때마다 선언한 스키마를 저장된 것과 비교합니다. 버전이 같은데 내용이 다르면 `SCHEMA_MISMATCH`로 실패하고, 저장된 버전이 낮으면 마이그레이션하며, 높으면 `SCHEMA_TOO_NEW`로 실패합니다. 컬렉션이나 인덱스를 선언하는 순서만 바꾼 것은 변경이 아닙니다.

스키마는 만드는 동안이 아니라 파일을 열 때 검사합니다. 저장할 수 없는 스키마는 파일을 만들기 전에 `open`이 `INVALID_ARGUMENT`로 실패합니다. 컬렉션마다 지킬 규칙은 [`Collection`](./collection.md)에 있습니다. 스키마를 쓰는 예는 [컬렉션과 객체](../../guide/objects.md)에, 새 버전에서 무엇을 바꿀 수 있는지는 [마이그레이션](../../guide/migrations.md)에 있습니다.

```rust
use darudb::{Collection, OpenOptions, Schema, Type};

fn main() -> darudb::Result<()> {
    let schema = Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .field("title", Type::String)
                .field("author", Type::link("users"))
                .index("author"),
        );
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    db.close()
}
```

## 연관 함수

### new

```rust
pub fn new(version: u64) -> Self
```

컬렉션이 아직 없는 `version` 버전의 스키마를 만듭니다. 버전은 1부터 시작하고, 애플리케이션은 스키마를 바꿀 때마다 버전을 올립니다. 버전 0은 `open`이 `INVALID_ARGUMENT`로 실패합니다.

### decode

```rust
pub fn decode(bytes: &[u8]) -> Result<Self>
```

다른 언어에서 선언한 스키마를 읽습니다. 스키마는 파일이 저장하는 형식으로 인코딩돼 있고, ID는 인코딩한 쪽이 정합니다. 언어 바인딩은 이렇게 스키마를 만들고, Rust 프로그램은 `new`를 씁니다. 이 ID는 링크와 인덱스가 무엇을 가리키는지 잇는 데만 쓰이며, 파일은 컬렉션과 필드에 자기 ID를 따로 붙입니다. 디코딩되지 않거나 ID가 서로 맞지 않는 레코드는 `INVALID_ARGUMENT`로 실패하고, 읽은 스키마는 파일을 열 때 다른 스키마처럼 검사합니다. 인코딩 형식은 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-stored-schema)에 있습니다.

## 메서드

### collection

```rust
pub fn collection(mut self, collection: Collection) -> Self
```

`collection`을 더합니다. 이름이 같은 컬렉션이 둘이면 `open`이 `INVALID_ARGUMENT`로 실패합니다.
