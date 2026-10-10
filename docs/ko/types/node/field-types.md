---
title: 필드 타입
order: 6
group: objects
pageClass: reference-page
---

# 필드 타입

필드 타입은 [`t`](../../api/node/t.md)가 스키마의 필드에 쓰라고 만들어 주는 타입이고, 타입마다 그 필드에 붙일 수 있는 수식 메서드가 있습니다.

필드 타입은 엔진에 필드를 설명하고, 타입 검사기만 아는 타입 매개변수로 TypeScript에도 필드를 설명합니다. `T`는 데이터베이스에서 읽은 객체에 든 값, `M`은 필드의 [모드](#fieldmode), `I`는 쓸 때 넘기는 값입니다. 수식 메서드는 언제나 새 타입을 돌려주고 원래 타입은 건드리지 않으므로, 타입 하나에서 여러 필드를 시작해도 됩니다. 엔진이 거부할 수식 메서드는 타입에 아예 없습니다. JavaScript에서 그래도 부르면 대부분은 그 자리에서, 나머지는 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다.

| 빌더                 | 타입                                                                  |
| -------------------- | --------------------------------------------------------------------- |
| `t.bool()`           | `FieldType<boolean>`                                                  |
| `t.int()`            | `KeyableType<number>`                                                 |
| `t.bigint()`         | `KeyableType<bigint, bigint \| number>`                               |
| `t.float()`          | `FieldType<number>`                                                   |
| `t.string()`         | `KeyableType<string>`                                                 |
| `t.bytes()`          | `KeyableType<Uint8Array>`                                             |
| `t.link(collection)` | `LinkType`                                                            |
| `t.list(element)`    | `FieldType<T[], 'required', I[]>`, 링크의 리스트는 `FieldType<Key[]>` |
| `t.object(fields)`   | `EmbeddedType<EmbeddedOf<F>, EmbeddedInputOf<F>>`                     |

```ts
import { collection, t } from 'darudb';

const users = collection({
  name: t.string(),
  email: t.string().optional().unique(),
  age: t.int().default(0).index(),
  team: t.link('teams').optional(),
  address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
});
```

이런 필드에서 TypeScript가 어떤 객체 타입을 끌어내는지는 [객체 타입](./object-types.md)에 있습니다.

## FieldMode

```ts
type FieldMode = 'required' | 'optional' | 'default';
```

필드가 값을 어떻게 담는지 나타냅니다. 모든 타입은 `'required'`로 시작합니다.

- **`'required'`**: 쓰는 객체마다 이 필드가 있어야 하고, null이 되지 않습니다.
- **`'optional'`**: null일 수 있고, 빠지면 null이 됩니다. `optional()`이 이 모드로 바꿉니다.
- **`'default'`**: 필수지만, 빠지면 기본값이 들어갑니다. `default(value)`가 이 모드로 바꿉니다.

## FieldType

```ts
interface FieldType<T, M extends FieldMode = 'required', I = T> extends Typed<T, M, false, I> {
  optional(): FieldType<T, 'optional', I>;
  default(value: I): FieldType<T, 'default', I>;
  index(): FieldType<T, M, I>;
  unique(): FieldType<T, M, I>;
}
```

평범한 필드의 타입입니다. `t.bool()`, `t.float()`, `t.list(element)`가 만들고, [KeyableType](#keyabletype)의 수식 메서드도 이 타입을 돌려줍니다.

- **`optional()`**: 필드가 null일 수 있고, 빠지면 null이 됩니다.
- **`default(value)`**: 필드가 빠지면 `value`가 들어갑니다. 필드의 타입과 맞지 않는 값은 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다.
- **`index()`**: 엔진이 필드에 인덱스를 두어, 이 필드로 찾는 쿼리가 모든 객체 대신 인덱스를 읽게 합니다. 리스트 필드의 인덱스에는 원소마다 항목이 생깁니다.
- **`unique()`**: 인덱스를 두고, 값이 같은 객체가 또 들어오면 `DUPLICATE_KEY`로 거부합니다. null은 몇 개가 있어도 됩니다.

리스트의 원소는 수식 메서드를 붙이지 않은 타입이어야 합니다. 원소가 선택 필드이거나 기본값이 있으면 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패하고, 리스트의 리스트도 마찬가지입니다.

## KeyableType

```ts
interface KeyableType<T, I = T> extends FieldType<T, 'required', I> {
  primaryKey(): KeyType<T, I>;
}
```

기본 키가 될 수 있는 필드의 타입으로, `t.int()`, `t.bigint()`, `t.string()`, `t.bytes()`가 만듭니다. `FieldType`의 수식 메서드가 모두 있고, 필드를 컬렉션의 기본 키로 삼는 `primaryKey()`가 더 있습니다. `primaryKey()`는 다른 수식 메서드보다 먼저 부르세요. 다른 수식 메서드는 `primaryKey()`가 없는 `FieldType`을 돌려줍니다.

컬렉션의 키 필드는 하나까지이고, 둘이면 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다. 키 필드가 없는 컬렉션에는 엔진이 1부터 번호를 매기는 `id`가 생기며, 자세한 내용은 [`collection`](../../api/node/collection.md)에 있습니다.

## KeyType

```ts
interface KeyType<T, I = T> extends Typed<T, 'required', true, I> {
  index(): KeyType<T, I>;
  unique(): KeyType<T, I>;
}
```

`primaryKey()`가 만든 기본 키 필드입니다. 필수이고, 기본값이 없으며, 선택 필드가 될 수 없습니다. 세 번째 타입 매개변수 `true`를 보고 [ObjectOf](./object-types.md#objectof)는 그 컬렉션의 객체에 엔진이 매긴 `id`가 없다는 것을 압니다. 수식 메서드는 `index()`와 `unique()`만 남습니다.

## LinkType

```ts
interface LinkType<M extends FieldMode = 'required'> extends Typed<Key, M, false, Key> {
  optional(): LinkType<'optional'>;
  index(): LinkType<M>;
  unique(): LinkType<M>;
}
```

링크는 `t.link(collection)`이 정한 컬렉션에 있는 객체의 기본 키를 담습니다. 그 컬렉션은 링크가 속한 컬렉션 자신이어도 됩니다. 값은 그 컬렉션의 키 타입에 맞는 [Key](./key.md)로 읽고 씁니다. `optional()`, `index()`, `unique()`가 있고, 기본값은 객체 하나를 가리키게 되므로 없습니다. 없는 객체를 가리켜도 되지만, 스키마에 없는 컬렉션을 가리키면 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다. `t.list(t.link(collection))`은 여러 객체를 가리키는 링크입니다.

## EmbeddedType

```ts
interface EmbeddedType<T, I, M extends FieldMode = 'required'> extends Typed<T, M, false, I> {
  optional(): EmbeddedType<T, I, 'optional'>;
}
```

필드가 따로 있는 내장 객체로, `t.object(fields)`가 만듭니다. 기본 키도 컬렉션도 없고, 담고 있는 객체와 함께 통째로 읽고 씁니다. 수식 메서드는 `optional()` 하나뿐입니다. 필드마다 기본값이 따로 있으므로 내장 객체 자체의 기본값은 없고, 인덱스도 없습니다. 안에 든 필드에 인덱스를 두거나 키로 삼아도 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패하고, 리스트는 내장 객체를 담을 수 없습니다. 쿼리에서는 `address.city` 같은 경로로 내장 객체의 필드에 닿습니다.

`T`는 필드의 [EmbeddedOf](./object-types.md#embeddedof)이고, `I`는 [EmbeddedInputOf](./object-types.md#embeddedinputof)입니다.

## AnyField

```ts
type AnyField = Typed<any, FieldMode, boolean, any>;
```

모든 종류의 필드를 다루는 코드에 쓰는, 아무 필드의 타입입니다. `Typed<T, M, K, I>`는 모든 필드 타입이 확장하는 인터페이스이고, `K`는 필드가 기본 키인지 나타냅니다. `Typed`의 멤버 넷은 타입 검사기에만 있고, 실행 중인 필드 타입에는 없습니다.

## Fields

```ts
type Fields = Record<string, AnyField>;
```

컬렉션이나 내장 객체의 필드를 이름별로 모은 것으로, [`collection`](../../api/node/collection.md)과 `t.object`가 받습니다. 객체 타입들은 `Fields`를 타입 매개변수로 받습니다.
