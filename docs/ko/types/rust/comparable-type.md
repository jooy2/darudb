---
title: ComparableType
order: 6.6
group: objects
pageClass: reference-page
---

# ComparableType

`ComparableType`과 그 옆의 트레이트는 [`FieldOf`](./field-of.md)를 거쳐 필드의 Rust 타입이 어떤 조건과 경로, 집계를 허용하는지 정합니다. 모두 크레이트가 구현하며, 크레이트 밖의 타입은 구현할 수 없습니다.

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

| 트레이트 | 허용하는 것 | 구현한 타입 |
| --- | --- | --- |
| `ComparableType` | `eq`, `ne`, `is_in` | `bool`, `i64`, `f64`, `String`, `Vec<u8>`, `Link<T>`와 그 `Option` |
| `OrderedType` | `lt`, `le`, `gt`, `ge`, `between`, 그리고 `min`과 `max` | `i64`, `f64`, `String`, `Vec<u8>`와 그 `Option` |
| `NumberType` | `sum`과 `avg` | `i64`, `f64`와 그 `Option` |
| `TextType` | `contains`, `starts_with`, `ends_with` | `String`, `Option<String>` |
| `NestedType` | `then` | 내장 객체 타입, `Link<T>`와 그 `Option` |
| `LinkType` | `any`, `any_where` | `Link<T>`, `Option<Link<T>>`, `Vec<Link<T>>` |

- **`Compared`**는 조건이 받는 값의 타입입니다. 필드 자신의 타입이고, 링크라면 대상 컬렉션의 키 타입, `Option`이라면 안쪽 타입의 것입니다. `min`과 `max`도 이 타입을 돌려줍니다.
- **`Sum`**은 `sum`이 돌려주는 타입으로, 정수 필드는 `i64`, 실수 필드는 `f64`입니다.
- **`Fields`**는 다음 단계를 상수로 가리키는 타입입니다. 내장 객체 타입이거나, `Link<T>`라면 `T`입니다.
- **`Target`**은 링크 필드가 가리키는 컬렉션 타입입니다.

목록 필드 `Vec<E>`에는 원소를 찾는 `contains`만 있습니다. 목록에 건 조건은 원소 하나라도 맞으면 참이기 때문입니다.

## IntoPath

```rust
pub trait IntoPath {}
```

[`Query::sort_by`](../../api/rust/query.md#sort-by)가 정렬하는 필드입니다. `&str`, `String`, `&String`으로 쓴 이름이나 점으로 이은 경로, 또는 `FieldOf`나 `PathOf`입니다.
