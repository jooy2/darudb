---
title: StoredSchema
order: 18
group: objects
pageClass: reference-page
---

# StoredSchema

`StoredSchema`는 [`Database.storedSchema`](../../api/node/database.md#storedschema)가 일반 데이터로 돌려주는 스키마입니다.

```ts
interface StoredSchema {
  readonly version: number;
  readonly collections: readonly StoredCollection[];
}
```

[`schema`](../../api/node/schema.md)와 [`collection`](../../api/node/collection.md), [`t`](../../api/node/t.md)로 선언한 내용을 파일이 붙이는 ID 없이 담습니다. 버전과 컬렉션 목록이 있고, 컬렉션마다 필드와 기본 키, 인덱스가 있습니다. 데이터베이스를 열 때 준 스키마를 나타내고, 스키마 없이 열었으면 파일에 저장된 스키마를 나타냅니다. 직접 쓰지 않은 파일을 여는 프로그램은 여기서 컬렉션을 알아냅니다. 모든 부분이 얼려 있어 바꿀 수 없습니다.

```ts
const db = Database.open('app.darudb');

for (const collection of db.storedSchema?.collections ?? []) {
  for (const field of collection.fields) {
    console.log(`${collection.name}.${field.name}`, field.type.kind, field.optional);
  }
}
```

## StoredCollection

```ts
interface StoredCollection {
  readonly name: string;
  readonly key: string;
  readonly autoIncrement: boolean;
  readonly fields: readonly StoredField[];
  readonly indexes: readonly StoredIndex[];
}
```

컬렉션 하나입니다. `key`는 기본 키 필드의 이름이고, 자동 증가 키라면 `id`입니다. `autoIncrement`는 기본 키가 엔진이 매기는 int `id`인지 알려 줍니다. `fields`는 파일에 저장된 순서입니다. 기본 키 필드도 여기에 들어 있지만, 자동 증가 키의 `id`는 선언한 필드가 아니므로 빠집니다.

## StoredField

```ts
interface StoredField {
  readonly name: string;
  readonly type: StoredType;
  readonly optional: boolean;
  readonly default?: unknown;
  readonly cascade: boolean;
}
```

컬렉션이나 내장 객체의 필드입니다. `optional`은 필드가 null일 수 있는지 알려 주며, 객체에서 빠지면 null이 됩니다. `default`는 필수 필드가 객체에서 빠졌을 때 들어가는 값이고, 기본값이 없는 필드에는 없습니다. `cascade`는 링크가 가리키는 객체를 지울 때 그 링크를 가진 객체도 지우는지 알려 줍니다.

## StoredIndex

```ts
interface StoredIndex {
  readonly field: string;
  readonly fields: readonly string[];
  readonly unique: boolean;
}
```

컬렉션의 인덱스입니다. 인덱스를 둔 필드들을 순서대로 담고, 각 필드는 이름이거나 `address.city` 같은 경로입니다. `unique`는 모든 필드의 값이 같은 두 객체를 거부하는 고유 인덱스인지 알려 줍니다. `field`는 `fields`의 첫 필드이며, `index()`나 `unique()`로 선언한 인덱스에는 이 필드 하나뿐입니다.

## StoredType

```ts
type StoredType =
  | { readonly kind: 'bool' | 'int' | 'float' | 'string' | 'bytes' }
  | { readonly kind: 'link'; readonly collection: string }
  | { readonly kind: 'list'; readonly element: StoredType }
  | { readonly kind: 'object'; readonly fields: readonly StoredField[] };
```

필드의 타입이며, `kind`가 종류를 나타냅니다. 링크는 기본 키를 담는 대상 컬렉션을, 목록은 원소의 타입을, 내장 객체는 자기 필드를 함께 담습니다. 목록의 원소는 스칼라 타입이나 링크입니다. 파일은 int 타입을 하나만 저장하므로 `t.int()`로 선언했든 `t.bigint()`로 선언했든 int 필드는 `int`입니다. 그래서 스키마 없이 연 데이터베이스는 그 값을 2^53까지는 number로, 그보다 크면 `bigint`로 읽습니다.
