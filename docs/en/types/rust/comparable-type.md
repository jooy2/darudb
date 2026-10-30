---
title: ComparableType
order: 6.6
group: objects
pageClass: reference-page
---

# ComparableType

`ComparableType` and the traits beside it say which conditions, paths and aggregates a field's Rust type allows through [`FieldOf`](./field-of.md). The crate implements them all, and no type outside it can.

```rust
pub trait ComparableType {
    type Compared;
}

pub trait OrderedType: ComparableType {}

pub trait NumberType: OrderedType {
    type Sum;
}

pub trait TextType: ComparableType<Compared = String> {}

pub trait NestedType {
    type Fields;
}

pub trait LinkType {
    type Target: CollectionType;
}
```

| Trait | Allows | Implemented for |
| --- | --- | --- |
| `ComparableType` | `eq`, `ne`, `is_in` | `bool`, `i64`, `f64`, `String`, `Vec<u8>`, `Link<T>`, and `Option` of one |
| `OrderedType` | `lt`, `le`, `gt`, `ge`, `between`; `min` and `max` | `i64`, `f64`, `String`, `Vec<u8>`, and `Option` of one |
| `NumberType` | `sum` and `avg` | `i64`, `f64`, and `Option` of either |
| `TextType` | `contains`, `starts_with`, `ends_with` | `String`, `Option<String>` |
| `NestedType` | `then` | an embedded object type, `Link<T>`, and `Option` of either |
| `LinkType` | `any`, `any_where` | `Link<T>`, `Option<Link<T>>`, `Vec<Link<T>>` |

- **`Compared`** is the type of the value a condition takes: the field's own type, the key type of the collection linked to for a link, and the inner type's for an `Option`. It is also what `min` and `max` return.
- **`Sum`** is what `sum` returns: `i64` for an int field and `f64` for a float field.
- **`Fields`** is the type whose constants name the step after it: the embedded object type, or `T` for a `Link<T>`.
- **`Target`** is the collection type a link field links to.

A list field, `Vec<E>`, has `contains` for an element and nothing else of these, since a condition on a list holds for any element.

## IntoPath

```rust
pub trait IntoPath {}
```

A field [`Query::sort_by`](../../api/rust/query.md#sort-by) sorts by: a name or a dotted path as a `&str`, a `String` or a `&String`, or a `FieldOf` or a `PathOf`.
