---
title: FieldOf
order: 6.5
group: objects
pageClass: reference-page
---

# FieldOf

`FieldOf<T, V>` names a field of the collection type `T`, or of the embedded object type `T`, whose values are of type `V`, so that an update or a query names the field through the type rather than by a name in a string.

```rust
pub struct FieldOf<T, V>
```

[`#[derive(Object)]`](../../api/rust/derive.md#object) declares one for each field of the struct, as an associated constant named after the field in capitals, with the field's visibility: `User::AGE` for a field `age`, of type `FieldOf<User, i64>`. [`TypedWriter::set`](../../api/rust/typed-writer.md#set) and [`Change::set`](../../api/rust/change.md#set) take it with a value of type `V`, so that a value of another type does not compile.

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

A constant holds the field's place among the struct's fields, as [`CollectionType::write_field`](./collection-type.md) counts them, the auto-increment `id` first when the collection has one, and the field's name in the collection. A hand-written `CollectionType` makes its own with `FieldOf::new`.

## Methods

### new

```rust
pub const fn new(slot: usize, name: &'static str) -> Self
```

The field numbered `slot` in `T`, named `name` in the collection. A condition on it gives the engine `name`.

### slot

```rust
pub const fn slot(self) -> usize
```

The field's number in `T`.

### name

```rust
pub const fn name(self) -> &'static str
```

The field's name in the collection: `photo` for a field declared with `#[darudb(rename = "photo")]`.

## Conditions

Each method makes a [`Filter`](../../api/rust/filter.md) on the field, the one its name in a string would make. Which ones a field has depends on its type, through the traits on the [ComparableType](./comparable-type.md) page.

| Method | For a field of type | Filter |
| --- | --- | --- |
| `eq(value)`, `ne(value)` | `bool`, `i64`, `f64`, `String`, `Vec<u8>`, `Link<T>`, and `Option` of one | [`Filter::eq`](../../api/rust/filter.md#eq), [`Filter::ne`](../../api/rust/filter.md#ne) |
| `is_in(values)` | the same | [`Filter::is_in`](../../api/rust/filter.md#is-in) |
| `lt`, `le`, `gt`, `ge` | `i64`, `f64`, `String`, `Vec<u8>`, and `Option` of one | [`Filter::lt`](../../api/rust/filter.md#lt) and the others |
| `between(low, high)` | the same | [`Filter::between`](../../api/rust/filter.md#between) |
| `contains(text)`, `starts_with(text)`, `ends_with(text)` | `String`, `Option<String>` | [`Filter::contains`](../../api/rust/filter.md#contains) and the others |
| `contains(element)` | `Vec<E>` | [`Filter::contains`](../../api/rust/filter.md#contains) on a list |
| `is_null()`, `is_not_null()` | any | [`Filter::is_null`](../../api/rust/filter.md#is-null), [`Filter::is_not_null`](../../api/rust/filter.md#is-not-null) |
| `any()`, `any_where(filter)` | `Link<U>`, `Option<Link<U>>`, `Vec<Link<U>>`, of a collection type | [`Filter::linked`](../../api/rust/filter.md#linked), [`Filter::linked_where`](../../api/rust/filter.md#linked-where) |

A value converts into the field's own type: `User::AGE.ge(18)` takes any integer that converts into an `i64`, `User::NAME.eq("Ada")` a `&str`, and a link the linked collection's key. The value of an `Option` field is its inner type's; null is `is_null`. `ignore_case` and the others of `Filter` apply to what these return: `User::NAME.eq("ada").ignore_case()`.

`any` holds for an object of the collection linked to that some object of `T` links to through this field: on a query of users, `Post::AUTHOR.any()` finds those with a post, and `Post::AUTHOR.any_where(Post::YEAR.ge(2020))` those with a recent one.

### then

```rust
pub fn then<W>(self, next: FieldOf<V::Fields, W>) -> PathOf<T, W>
where
    V: NestedType,
```

The path from this field on to `next`, a field of the embedded object it holds, or of the object it links to: `User::HOME.then(Address::CITY)`, `Post::AUTHOR.then(User::NAME)`. [`#[derive(Embedded)]`](../../api/rust/derive.md#embedded) declares the constants of an embedded object.

## PathOf

```rust
pub struct PathOf<T, V>
```

A path from `T` through embedded objects and links to a field whose values are of type `V`, which `then` makes. It has the condition methods and `then` of `FieldOf`, and `names`, the names of the fields it goes through. [`Query::sort_by`](../../api/rust/query.md#sort-by) sorts by it, and the [typed aggregates](../../api/rust/typed-reader.md#sum) take it.

```rust
pub fn names(&self) -> &[&'static str]
```
