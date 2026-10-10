---
title: FieldType
order: 5
group: objects
pageClass: reference-page
---

# FieldType

`FieldType`은 타입 객체 필드의 Rust 타입입니다. 스키마에서 그 필드를 어떻게 선언하는지, 레코드에 어떻게 쓰고 레코드에서 어떻게 읽는지를 정합니다.

```rust
pub trait FieldType: Sized {
    const OPTIONAL: bool = false;

    fn kind() -> Type;
    fn write(&self, value: ValueWriter<'_>) -> Result<()>;
    fn read(value: ValueReader<'_>) -> Result<Self>;
}
```

크레이트는 아래 타입에 이 트레이트를 구현하고, 내장 객체에는 [`#[derive(Embedded)]`](../../api/rust/derive.md#embedded)가 구현합니다.

| Rust 타입                 | 필드 타입                                |
| ------------------------- | ---------------------------------------- |
| `bool`                    | `Type::Bool`                             |
| `i64`                     | `Type::Int`                              |
| `f64`                     | `Type::Float`                            |
| `String`                  | `Type::String`                           |
| `Vec<u8>`                 | `Type::Bytes`                            |
| `Vec<T>`, `T`는 요소 타입 | `Type::list(T::kind())`                  |
| [`Link<T>`](./link.md)    | `Type::link(T::COLLECTION)`              |
| `Option<T>`               | `T`의 타입, 선택 필드                    |
| `Embedded`를 붙인 구조체  | 구조체의 필드를 담은 `Type::object(...)` |

`Option<T>`는 `OPTIONAL`이 `true`입니다. `None`을 쓰면 필드를 레코드에서 빼고, null을 읽으면 `None`이 됩니다. `Vec<u8>`은 바이트이므로 작은 정수의 목록은 `Vec<i64>`로 씁니다.

## ElementType

```rust
pub trait ElementType: FieldType {}
```

목록이 담을 수 있는 타입입니다. `bool`, `i64`, `f64`, `String`, `Vec<u8>`, `Link<T>`가 있습니다. 목록에는 null도 내장 객체도 들어가지 않습니다.

## KeyType

```rust
pub trait KeyType: FieldType {
    fn to_value(&self) -> Value;
    fn from_value(value: Value) -> Result<Self>;
}
```

기본 키가 가질 수 있는 타입입니다. `i64`, `String`, `Vec<u8>`이 있습니다.

## ValueWriter

```rust
pub struct ValueWriter<'a>
```

레코드의 값 하나를 씁니다. 필드의 값, 목록의 요소, 링크의 키가 모두 여기로 쓰입니다. 메서드마다 라이터를 가져가므로 값은 한 번만 씁니다.

- `null()`: 값이 없으므로 선택 필드를 레코드에서 뺍니다. 목록의 요소에 쓰면 `INVALID_ARGUMENT`로 실패합니다.
- `bool(value)`, `int(value)`, `float(value)`, `string(&str)`, `bytes(&[u8])`.
- [`ElementType`](#elementtype)의 목록은 `list(&[T])`, 내장 객체는 `object(&E)`, 링크는 가리키는 객체의 키로 `link(&K)`를 씁니다.

## ValueReader

```rust
pub struct ValueReader<'a>
```

레코드에서 빌려 온 값 하나입니다. 레코드에 없는 필드는 기본값이나 null로 읽힙니다.

- `is_null()`.
- `bool()`, `int()`, `float()`, `str()`, `bytes()`. 마지막 둘은 레코드에서 빌려 옵니다.
- `list::<T>()`, `object::<E>()`, `link::<K>()`.

메서드와 타입이 다른 값이나 UTF-8이 아닌 텍스트는 `CORRUPTED`로 실패합니다.
