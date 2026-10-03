---
title: FieldType
order: 5
---

# FieldType

`FieldType` is the Rust type of a field of a typed object: how a schema declares it, and how it is written into a record and read from one.

```rust
pub trait FieldType: Sized {
    const OPTIONAL: bool = false;

    fn kind() -> Type;
    fn write(&self, value: ValueWriter<'_>) -> Result<()>;
    fn read(value: ValueReader<'_>) -> Result<Self>;
}
```

The crate implements it for these types, and [`#[derive(Embedded)]`](../../api/rust/derive.md#embedded) for an embedded object:

| Rust type                | Field type                                   |
| ------------------------ | -------------------------------------------- |
| `bool`                   | `Type::Bool`                                 |
| `i64`                    | `Type::Int`                                  |
| `f64`                    | `Type::Float`                                |
| `String`                 | `Type::String`                               |
| `Vec<u8>`                | `Type::Bytes`                                |
| `Vec<T>`, `T` an element | `Type::list(T::kind())`                      |
| [`Link<T>`](./link.md)   | `Type::link(T::COLLECTION)`                  |
| `Option<T>`              | `T`'s type, optional                         |
| A struct with `Embedded` | `Type::object(...)` with the struct's fields |

`OPTIONAL` is `true` for `Option<T>`, which writes `None` by leaving the field out and reads null as `None`. A list of small integers is a `Vec<i64>`, since `Vec<u8>` is bytes.

## ElementType

```rust
pub trait ElementType: FieldType {}
```

A type a list may hold: `bool`, `i64`, `f64`, `String`, `Vec<u8>` and `Link<T>`. A list holds no null and no embedded object.

## KeyType

```rust
pub trait KeyType: FieldType {
    fn to_value(&self) -> Value;
    fn from_value(value: Value) -> Result<Self>;
}
```

A type a primary key may have: `i64`, `String` and `Vec<u8>`.

## ValueWriter

```rust
pub struct ValueWriter<'a>
```

Writes one value of a record: a field's, an element of a list, or the key of a link. Each method takes the writer, so a value is written once.

- `null()`: no value, so an optional field is left out. An element of a list fails with `INVALID_ARGUMENT`.
- `bool(value)`, `int(value)`, `float(value)`, `string(&str)`, `bytes(&[u8])`.
- `list(&[T])` for an [`ElementType`](#elementtype), `object(&E)` for an embedded object, and `link(&K)` with the key of the object linked to.

## ValueReader

```rust
pub struct ValueReader<'a>
```

One value of a record, borrowed from it. A field the record leaves out reads as its default, or as null.

- `is_null()`.
- `bool()`, `int()`, `float()`, `str()` and `bytes()`, the last two borrowed from the record.
- `list::<T>()`, `object::<E>()` and `link::<K>()`.

A value of another type than the method's, or text that is not UTF-8, fails with `CORRUPTED`.
