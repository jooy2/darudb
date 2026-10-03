---
title: Type
order: 3
counterpart: /api/node/t
---

# Type

`Type` is the type of a field, which a schema gives every field of a collection and of an embedded object.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Type
```

A type is passed to [`Collection`](../../api/rust/collection.md) and [`Embedded`](../../api/rust/embedded.md) when a field is declared, with the field's name. The scalar types are written as variants, `Type::String`, and the others with the associated functions below, `Type::link("users")`. [Value](./value.md#field-types) says which value each type holds.

```rust
use darudb::{Collection, Embedded, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .primary_key("handle", Type::String)
                .field("name", Type::String)
                .with_default("age", Type::Int, 0),
        )
        .collection(
            Collection::new("posts")
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .optional(
                    "place",
                    Type::object(Embedded::new().field("city", Type::String).optional("zip", Type::String)),
                )
                .index("author")
                .index("tags"),
        )
}
```

## Variants

| Variant            | A field of this type holds                           |
| ------------------ | ---------------------------------------------------- |
| `Bool`             | `false` or `true`                                    |
| `Int`              | A signed 64-bit integer                              |
| `Float`            | A 64-bit floating-point number                       |
| `String`           | UTF-8 text                                           |
| `Bytes`            | Any bytes                                            |
| `Link(String)`     | The primary key of an object in the named collection |
| `List(Box<Type>)`  | A list of values of a scalar type or of links        |
| `Object(Embedded)` | An embedded object with fields of its own            |

## Associated functions

### link

```rust
pub fn link(collection: impl Into<String>) -> Self
```

A link to an object of `collection`, which has to be a collection of the same schema. The field holds that object's primary key, and the object does not have to exist.

### list

```rust
pub fn list(element: Type) -> Self
```

A list of values of `element`, which is a scalar type or a link. A list of lists or of embedded objects is refused.

### object

```rust
pub fn object(embedded: Embedded) -> Self
```

An embedded object with the fields of `embedded`. It is stored inside its object's record and has no key of its own.

## Where each type may be used

The schema is checked when the database is opened with it, and a rule broken fails the open with `INVALID_ARGUMENT`.

- **Primary key.** A field named with `Collection::primary_key` is an `Int`, a `String` or `Bytes`. A collection without one gets an `Int` field called `id`, which the engine numbers.
- **Index.** A scalar field or a link can be indexed, and so can a list of them, which gets an entry for each element. An embedded object cannot.
- **Default.** The value given to `with_default` is a value of the field's type, or for a list field a list of such values. A link has no default, and an embedded object has its own fields' defaults instead.
- **Link target.** The collection a link names has to be declared in the same schema.
