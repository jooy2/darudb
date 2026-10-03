---
title: CollectionType
order: 4
---

# CollectionType

`CollectionType` is a Rust type whose values are the objects of a collection, and `EmbeddedType` one whose values are embedded objects. [`#[derive(Object)]`](../../api/rust/derive.md) and [`#[derive(Embedded)]`](../../api/rust/derive.md#embedded) implement them; this page is for implementing them by hand, or for writing code generic over them.

```rust
pub trait CollectionType: Sized + 'static {
    type Key: KeyType;
    const COLLECTION: &'static str;

    fn collection() -> Collection;
    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()>;
    fn read(fields: FieldReader<'_>) -> Result<Self>;
}
```

- **`Key`** is the primary key's type: `i64` for an auto-increment, or the key field's [`KeyType`](./field-type.md#keytype).
- **`COLLECTION`** is the collection's name, the one `collection` declares. A [`Link`](./link.md) names its target's collection through it, without declaring the collection, which a link from a collection to its own objects would otherwise do without end.
- **`collection`** declares the collection, as [`Collection`](../../api/rust/collection.md) builds one.
- **Slots.** The fields are numbered in the order `collection` declares them, from 0, with the auto-increment `id` first when the collection has one. `write_field` writes the field of a slot, and `read` gets each field once with its slot.

```rust
use darudb::{Collection, CollectionType, FieldReader, FieldType, Type, ValueWriter};

#[derive(Debug)]
struct Note {
    id: Option<i64>,
    text: String,
}

impl CollectionType for Note {
    type Key = i64;

    const COLLECTION: &'static str = "notes";

    fn collection() -> Collection {
        Collection::new("notes").field("text", Type::String)
    }

    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> darudb::Result<()> {
        match slot {
            0 => self.id.write(value),
            1 => self.text.write(value),
            _ => Ok(()),
        }
    }

    fn read(mut fields: FieldReader<'_>) -> darudb::Result<Self> {
        let (mut id, mut text) = (None, None);

        while let Some((slot, value)) = fields.next()? {
            match slot {
                0 => id = Some(FieldType::read(value)?),
                1 => text = Some(FieldType::read(value)?),
                _ => {}
            }
        }

        Ok(Self { id: fields.take(id)?, text: fields.take(text)? })
    }
}
```

A hand-written implementation cannot store what the schema does not allow: a typed write is checked by the engine as a write from any language is, and one that does not fit fails with `INVALID_ARGUMENT`.

## EmbeddedType

```rust
pub trait EmbeddedType: Sized + 'static {
    fn embedded() -> Embedded;
    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()>;
    fn read(fields: FieldReader<'_>) -> Result<Self>;
}
```

The same for an embedded object: `embedded` declares its fields as [`Embedded`](../../api/rust/embedded.md) does, and they are numbered in that order. A type that implements it also implements [`FieldType`](./field-type.md), with `ValueWriter::object` and `ValueReader::object`, so that a field can hold it.

## FieldReader

```rust
pub struct FieldReader<'a>
```

Gives the fields of a record one at a time: `next` returns each field's slot and its [`ValueReader`](./field-type.md#valuereader), and `None` once every field has come. A field the record leaves out, as a record written before the field existed does, comes with its default, or as null. A field the schema no longer has is stepped over.

- `next(&mut self) -> Result<Option<(usize, ValueReader<'a>)>>`
- `take<T>(&self, value: Option<T>) -> Result<T>`: the value a slot was given, which every slot is; `CORRUPTED` otherwise.

A record that is damaged fails with `CORRUPTED`, naming the file and the collection.
