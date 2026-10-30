---
title: FieldOf
order: 6.5
group: objects
pageClass: reference-page
---

# FieldOf

`FieldOf<T, V>`는 컬렉션 타입이나 내장 객체 타입 `T`에서 값의 타입이 `V`인 필드를 가리킵니다. 업데이트와 쿼리가 필드를 문자열 이름이 아니라 타입으로 가리키게 해 줍니다.

```rust
pub struct FieldOf<T, V>
```

[`#[derive(Object)]`](../../api/rust/derive.md#object)는 구조체의 필드마다 하나씩, 필드 이름을 대문자로 쓴 연관 상수를 필드와 같은 공개 범위로 선언합니다. `age` 필드라면 `User::AGE`이고 타입은 `FieldOf<User, i64>`입니다. [`TypedWriter::set`](../../api/rust/typed-writer.md#set)과 [`Change::set`](../../api/rust/change.md#set)은 이 상수와 `V` 타입의 값을 함께 받으므로, 다른 타입의 값을 넘기면 컴파일되지 않습니다.

```rust
use darudb::{Database, Object};

#[derive(Object, Debug)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(index, default = 0)]
    age: i64,
}

fn birthday(db: &Database, id: i64, age: i64) -> darudb::Result<bool> {
    let mut txn = db.begin_write()?;
    let updated = txn.collection_of::<User>()?.set(id, User::AGE, age + 1)?;

    txn.commit()?;
    Ok(updated)
}
```

상수는 [`CollectionType::write_field`](./collection-type.md)가 세는 구조체 필드의 번호와 컬렉션에서의 필드 이름을 담습니다. 컬렉션이 자동 증가 키를 쓰면 `id`가 0번입니다. `CollectionType`을 직접 구현한다면 `FieldOf::new`로 상수를 만듭니다.

## 메서드

### new

```rust
pub const fn new(slot: usize, name: &'static str) -> Self
```

`T`에서 `slot`번이고 컬렉션에서 `name`이라는 이름인 필드입니다. 이 필드에 건 조건은 엔진에 `name`을 넘깁니다.

### slot

```rust
pub const fn slot(self) -> usize
```

이 필드의 `T` 안에서의 번호입니다.

### name

```rust
pub const fn name(self) -> &'static str
```

컬렉션에서의 필드 이름입니다. `#[darudb(rename = "photo")]`로 선언한 필드라면 `photo`입니다.

## 조건

메서드마다 이 필드에 건 [`Filter`](../../api/rust/filter.md)를 만들며, 문자열로 이름을 썼을 때와 같은 필터입니다. 필드가 어떤 메서드를 갖는지는 [ComparableType](./comparable-type.md) 페이지의 트레이트를 거쳐 필드의 타입이 정합니다.

| 메서드 | 필드 타입 | 필터 |
| --- | --- | --- |
| `eq(value)`, `ne(value)` | `bool`, `i64`, `f64`, `String`, `Vec<u8>`, `Link<T>`와 그 `Option` | [`Filter::eq`](../../api/rust/filter.md#eq), [`Filter::ne`](../../api/rust/filter.md#ne) |
| `is_in(values)` | 위와 같음 | [`Filter::is_in`](../../api/rust/filter.md#is-in) |
| `lt`, `le`, `gt`, `ge` | `i64`, `f64`, `String`, `Vec<u8>`와 그 `Option` | [`Filter::lt`](../../api/rust/filter.md#lt) 등 |
| `between(low, high)` | 위와 같음 | [`Filter::between`](../../api/rust/filter.md#between) |
| `contains(text)`, `starts_with(text)`, `ends_with(text)` | `String`, `Option<String>` | [`Filter::contains`](../../api/rust/filter.md#contains) 등 |
| `contains(element)` | `Vec<E>` | 목록에 쓴 [`Filter::contains`](../../api/rust/filter.md#contains) |
| `is_null()`, `is_not_null()` | 모든 타입 | [`Filter::is_null`](../../api/rust/filter.md#is-null), [`Filter::is_not_null`](../../api/rust/filter.md#is-not-null) |
| `any()`, `any_where(filter)` | 컬렉션 타입의 `Link<U>`, `Option<Link<U>>`, `Vec<Link<U>>` | [`Filter::linked`](../../api/rust/filter.md#linked), [`Filter::linked_where`](../../api/rust/filter.md#linked-where) |

값은 필드 자신의 타입으로 바뀌는 값이면 됩니다. `User::AGE.ge(18)`은 `i64`로 바뀌는 정수를, `User::NAME.eq("Ada")`는 `&str`을, 링크는 대상 컬렉션의 키를 받습니다. `Option` 필드의 값은 안쪽 타입의 값이고, null은 `is_null`로 검사합니다. `ignore_case`처럼 `Filter`의 메서드는 이들이 돌려준 필터에 그대로 씁니다. `User::NAME.eq("ada").ignore_case()`처럼 씁니다.

`any`는 `T`의 어떤 객체가 이 필드로 가리키는, 대상 컬렉션의 객체에서 참입니다. 사용자 쿼리에서 `Post::AUTHOR.any()`는 글이 있는 사용자를, `Post::AUTHOR.any_where(Post::YEAR.ge(2020))`는 최근 글이 있는 사용자를 찾습니다.

### then

```rust
pub fn then<W>(self, next: FieldOf<V::Fields, W>) -> PathOf<T, W>
where
    V: NestedType,
```

이 필드에서 `next`로 이어지는 경로입니다. `next`는 이 필드가 담은 내장 객체나 이 필드가 가리키는 객체의 필드입니다. `User::HOME.then(Address::CITY)`, `Post::AUTHOR.then(User::NAME)`처럼 씁니다. 내장 객체의 상수는 [`#[derive(Embedded)]`](../../api/rust/derive.md#embedded)가 선언합니다.

## PathOf

```rust
pub struct PathOf<T, V>
```

`T`에서 내장 객체와 링크를 지나 값의 타입이 `V`인 필드에 닿는 경로로, `then`이 만듭니다. `FieldOf`의 조건 메서드와 `then`을 똑같이 갖고, 지나는 필드의 이름을 돌려주는 `names`가 있습니다. [`Query::sort_by`](../../api/rust/query.md#sort-by)는 경로로도 정렬하고, [타입 있는 집계](../../api/rust/typed-reader.md#sum)도 경로를 받습니다.

```rust
pub fn names(&self) -> &[&'static str]
```
