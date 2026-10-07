---
title: CollectionType
order: 4
---

# CollectionType

`CollectionType`은 값이 컬렉션의 객체가 되는 Rust 타입이고, `EmbeddedType`은 값이 내장 객체가 되는 Rust 타입입니다. [`#[derive(Object)]`](../../api/rust/derive.md)와 [`#[derive(Embedded)]`](../../api/rust/derive.md#embedded)가 구현합니다. 이 페이지는 직접 구현하거나 두 트레이트에 대해 제네릭 코드를 쓸 때 봅니다.

```rust
pub trait CollectionType: Sized + 'static {
    type Key: KeyType;
    const COLLECTION: &'static str;

    fn collection() -> Collection;
    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()>;
    fn read(fields: FieldReader<'_>) -> Result<Self>;
}
```

- **`Key`는** 기본 키의 타입입니다. 자동 증가 키면 `i64`이고, 아니면 키 필드의 [`KeyType`](./field-type.md#keytype)입니다.
- **`COLLECTION`은** `collection`이 선언하는 컬렉션 이름입니다. [`Link`](./link.md)는 컬렉션을 선언하지 않고 이 이름으로 대상 컬렉션을 가리킵니다. 자기 컬렉션의 객체를 가리키는 링크가 컬렉션을 선언하면 선언이 끝없이 이어지기 때문입니다.
- **`collection`은** [`Collection`](../../api/rust/collection.md)으로 컬렉션을 선언합니다.
- **슬롯.** 필드는 `collection`이 선언한 순서대로 0부터 번호를 받고, 자동 증가 `id`가 있으면 그 필드가 첫 번호입니다. `write_field`는 슬롯 번호로 필드를 쓰고, `read`는 필드마다 슬롯 번호와 함께 한 번씩 받습니다.

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

직접 구현해도 스키마가 허락하지 않는 값은 저장되지 않습니다. 타입으로 쓰는 쓰기도 다른 언어에서 오는 쓰기와 똑같이 엔진이 확인하고, 맞지 않으면 `INVALID_ARGUMENT`로 실패합니다.

## EmbeddedType

```rust
pub trait EmbeddedType: Sized + 'static {
    fn embedded() -> Embedded;
    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()>;
    fn read(fields: FieldReader<'_>) -> Result<Self>;
}
```

내장 객체용으로 같은 일을 합니다. `embedded`가 [`Embedded`](../../api/rust/embedded.md)로 필드를 선언하고, 필드 번호도 그 순서를 따릅니다. 이 트레이트를 구현하는 타입은 `ValueWriter::object`와 `ValueReader::object`로 [`FieldType`](./field-type.md)도 구현해서, 필드가 그 타입을 담을 수 있게 합니다.

## FieldReader

```rust
pub struct FieldReader<'a>
```

레코드의 필드를 하나씩 줍니다. `next`는 필드마다 슬롯 번호와 [`ValueReader`](./field-type.md#valuereader)를 돌려주고, 필드가 다 나오면 `None`을 돌려줍니다. 필드가 생기기 전에 쓴 레코드처럼 레코드에 없는 필드는 기본값이나 null로 나옵니다. 스키마에서 사라진 필드는 건너뜁니다.

- `next(&mut self) -> Result<Option<(usize, ValueReader<'a>)>>`
- `take<T>(&self, value: Option<T>) -> Result<T>`: 슬롯이 받은 값을 꺼냅니다. 모든 슬롯은 값을 받으므로, 받지 못했으면 `CORRUPTED`입니다.

손상된 레코드는 파일과 컬렉션을 밝히며 `CORRUPTED`로 실패합니다.
