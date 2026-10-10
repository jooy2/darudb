---
title: Link
order: 6
group: objects
pageClass: reference-page
---

# Link

`Link<T>` is a link to an object of `T`'s collection: the object's primary key, typed by the collection it points into.

```rust
pub struct Link<T: CollectionType>
```

A field of type `Link<T>` is a field of type `Type::link(T::COLLECTION)`, and `Vec<Link<T>>` is a list of links. A link to an object that does not exist, or no longer does, is allowed, and reads as the key it holds. A query follows a link with a path through it, such as `author.name`.

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

It implements `Clone`, `PartialEq`, `Eq`, `Hash` and `Debug` when the key does.

## Associated functions

### new

```rust
pub fn new(key: T::Key) -> Self
```

A link to the object whose primary key is `key`.

## Methods

### key

```rust
pub fn key(&self) -> &T::Key
```

The primary key of the object linked to.

### into_key

```rust
pub fn into_key(self) -> T::Key
```

The primary key, taken out of the link.
