---
title: 파생 매크로
order: 12
counterpart: /api/node/schema
---

# 파생 매크로

`#[derive(Object)]`는 구조체를 컬렉션의 객체로 만들고, `#[derive(Embedded)]`는 구조체를 내장 객체로 만듭니다. 스키마는 구조체에서 컬렉션을 선언하고, 트랜잭션은 레코드를 구조체로 바로 읽고 구조체를 레코드로 씁니다.

```toml
[dependencies]
darudb = { path = "../darudb/crates/darudb", features = ["derive"] }
```

두 매크로는 크레이트의 `derive` 기능을 켜야 쓸 수 있습니다. 매크로를 쓰지 않는 프로그램이 Rust 파서까지 빌드하지 않도록 기본으로는 꺼 두었습니다. 기능을 켜지 않아도 [`CollectionType`](../../types/rust/collection-type.md)과 [`FieldType`](../../types/rust/field-type.md) 트레이트는 직접 구현할 수 있습니다.

```rust
use darudb::{Collection, Embedded, Filter, Link, Object, OpenOptions, Query, Schema};

#[derive(Embedded, Debug, Clone, PartialEq)]
struct Address {
    city: String,
    #[darudb(rename = "zip")]
    postal_code: Option<String>,
}

#[derive(Object, Debug, Clone, PartialEq)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
    tags: Vec<String>,
    address: Option<Address>,
}

#[derive(Object, Debug, Clone, PartialEq)]
#[darudb(collection = "posts")]
struct Post {
    #[darudb(key)]
    slug: String,
    #[darudb(index)]
    author: Link<User>,
}

fn main() -> Result<(), darudb::Error> {
    let schema = Schema::new(1)
        .collection(Collection::of::<User>())
        .collection(Collection::of::<Post>());
    let db = OpenOptions::new().schema(schema).open("app.darudb")?;

    let mut txn = db.begin_write()?;
    let alice = txn.collection_of::<User>()?.insert(&User {
        id: None,
        name: "Alice".to_owned(),
        email: Some("alice@example.com".to_owned()),
        age: 31,
        tags: vec!["admin".to_owned()],
        address: Some(Address { city: "Seoul".to_owned(), postal_code: None }),
    })?;
    txn.collection_of::<Post>()?.insert(&Post {
        slug: "hello".to_owned(),
        author: Link::new(alice),
    })?;
    txn.commit()?;

    let read = db.begin_read()?;
    let adults: Vec<User> = read
        .collection_of::<User>()?
        .query(&Query::new().filter(Filter::ge("age", 18)))?;

    println!("{adults:?}");
    Ok(())
}
```

## Object

```rust
#[proc_macro_derive(Object, attributes(darudb))]
```

이름 있는 필드를 가졌고 제네릭 매개변수가 없는 구조체에 [`CollectionType`](../../types/rust/collection-type.md)을 구현합니다. 컬렉션은 [`Collection::of`](./collection.md#of)로 선언하고, 객체는 [`ReadTransaction::collection_of`](./read-transaction.md#collection-of)와 [`WriteTransaction::collection_of`](./write-transaction.md#collection-of)로 읽고 씁니다.

- **컬렉션 이름**은 구조체 이름을 그대로 씁니다. 구조체에 `#[darudb(collection = "name")]`을 붙이면 그 이름을 씁니다.
- **기본 키**는 `#[darudb(key)]`를 붙인 필드이고, 타입은 `i64`, `String`, `Vec<u8>` 가운데 하나입니다. 키 필드가 없으면 자동 증가 키를 쓰므로 구조체에 `id: Option<i64>` 필드가 있어야 합니다. 이 필드는 객체를 넣기 전까지 `None`이고, 넣을 때 다음 번호를 받습니다.
- **필드**는 구조체에 적힌 순서대로 선언되고, 타입은 Rust 타입에서 정해집니다. [`FieldType`](../../types/rust/field-type.md)을 보세요. `Option`이면 선택 필드이고, 나머지는 모두 필수 필드입니다.

## Embedded

```rust
#[proc_macro_derive(Embedded, attributes(darudb))]
```

이름 있는 필드를 가진 구조체에 [`EmbeddedType`](../../types/rust/collection-type.md#embeddedtype)과 [`FieldType`](../../types/rust/field-type.md)을 구현해, 객체나 다른 내장 객체의 필드가 그 구조체를 담을 수 있게 합니다. 필드는 [`Embedded::of`](./embedded.md#of)로 선언합니다. 내장 객체에는 키가 없으므로 필드에 `key`도 인덱스도 붙일 수 없습니다.

## 필드 속성

| 속성 | 뜻 |
| --- | --- |
| `#[darudb(key)]` | 이 필드가 기본 키입니다. `Object`에만 씁니다. |
| `#[darudb(index)]` | 이 필드에 인덱스를 둡니다. `Object`에만 씁니다. |
| `#[darudb(unique)]` | 같은 값을 가진 두 객체를 거부하는 인덱스를 둡니다. `Object`에만 씁니다. |
| `#[darudb(rename = "x")]` | 컬렉션에서 쓰는 필드 이름입니다. 없으면 Rust 필드 이름을 씁니다. |
| `#[darudb(default = 18)]` | 레코드에 필드가 없을 때 읽히는 값입니다. 필드는 필수가 됩니다. 키에는 붙일 수 없습니다. |

기본값에는 [`Value::from`](../../types/rust/value.md)이 받는 값이면 무엇이든 쓸 수 있습니다. 속성은 `#[darudb(index, default = 0)]`처럼 한 목록에 함께 적습니다.

## 읽고 쓸 때 확인하는 것

구조체와 저장된 컬렉션은 핸들과 타입마다 한 번, 트랜잭션이 그 타입으로 컬렉션에 처음 닿을 때 이름으로 맞춰 봅니다. 필드가 같고, 타입과 선택 필드 여부가 같고, 기본 키가 같아야 합니다. 다르면 `collection_of`가 맞지 않는 필드를 알려 주며 `INVALID_ARGUMENT`로 실패합니다. 스키마를 선언한 구조체는 당연히 맞고, 필드 순서만 다른 구조체도 맞습니다.

타입으로 쓰는 쓰기도 다른 쓰기와 똑같이 엔진이 확인합니다. 고유 인덱스에 이미 있는 값은 `DUPLICATE_KEY`로 실패하고, 트랜잭션은 그 전 상태 그대로 이어집니다. 구조체로 읽히지 않는 레코드는 손상된 파일에만 있고, `CORRUPTED`로 실패합니다.
