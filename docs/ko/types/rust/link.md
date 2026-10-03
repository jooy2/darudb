---
title: Link
order: 6
---

# Link

`Link<T>`는 `T` 컬렉션의 객체를 가리키는 링크입니다. 가리키는 객체의 기본 키를 담고, 어느 컬렉션을 가리키는지가 타입에 드러납니다.

```rust
pub struct Link<T: CollectionType>
```

`Link<T>` 필드는 `Type::link(T::COLLECTION)` 필드이고, `Vec<Link<T>>`는 링크의 목록입니다. 없는 객체나 지워진 객체를 가리키는 링크도 쓸 수 있고, 그런 링크는 담고 있는 키로 읽힙니다. 쿼리에서는 `author.name`처럼 링크를 거치는 경로로 링크를 따라갑니다.

```rust
use darudb::{Link, Object};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
}

#[derive(Object, Debug)]
#[darudb(collection = "posts")]
struct Post {
    #[darudb(key)]
    slug: String,
    #[darudb(index)]
    author: Link<User>,
    readers: Vec<Link<User>>,
}

fn main() {
    let post = Post { slug: "hello".to_owned(), author: Link::new(1), readers: Vec::new() };

    assert_eq!(*post.author.key(), 1);
}
```

키가 `Clone`, `PartialEq`, `Eq`, `Hash`, `Debug`를 구현하면 링크도 구현합니다.

## 연관 함수

### new

```rust
pub fn new(key: T::Key) -> Self
```

기본 키가 `key`인 객체를 가리키는 링크를 만듭니다.

## 메서드

### key

```rust
pub fn key(&self) -> &T::Key
```

가리키는 객체의 기본 키입니다.

### into_key

```rust
pub fn into_key(self) -> T::Key
```

링크에서 기본 키를 꺼냅니다.
