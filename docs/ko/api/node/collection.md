---
title: collection
order: 3
---

# collection

`collection`은 스키마에 들어갈 컬렉션 하나를 선언하며, 필드를 정하면 기본 키와 인덱스도 함께 정해집니다.

```ts
const collection: <F extends Fields>(fields: F, options?: CollectionOptions<F>) => Collection<F>;
```

`fields`의 값은 모두 [t](./t.md)로 만든 필드 타입이어야 하고, 아니면 곧바로 `INVALID_ARGUMENT`를 던집니다. 컬렉션 이름은 [schema](./schema.md)에 넣을 때 붙이는 이름입니다. 객체의 타입은 필드에서 나옵니다. 읽은 객체의 타입은 [ObjectOf](../../types/node/object-types.md)이고, 쓰는 객체의 타입은 `InsertOf`입니다.

```ts
import { collection, t } from 'darudb';

const teams = collection({ name: t.string().primaryKey(), city: t.string().optional() });
const users = collection({ name: t.string(), team: t.link('teams').optional() });
```

## 기본 키

객체마다 기본 키가 있습니다. `get`, `update`, `delete`가 이 키를 받고, 한 컬렉션에서 두 객체가 같은 키를 가질 수 없습니다.

- **키 필드.** `t.int()`, `t.bigint()`, `t.string()`, `t.bytes()` 중 하나에 `primaryKey()`를 붙인 필드가 키입니다. 키는 필수이고 기본값이 없으며, 객체의 키는 바뀌지 않습니다. 다른 키로 쓰면 다른 객체를 쓰는 것입니다. 이런 필드가 둘인 컬렉션은 열 때 `INVALID_ARGUMENT`로 실패합니다.
- **자동 `id`.** 키 필드가 없는 컬렉션에는 `id`라는 정수 필드가 생기고, number로 읽힙니다. `id` 없이 쓴 객체는 1부터 차례로 다음 번호를 받습니다. 객체를 지워도 한 파일 안에서 같은 번호를 두 번 주지 않습니다. `id`를 직접 넣어 쓴 객체는 그 번호를 그대로 쓰고, 그 뒤에 주는 번호는 그보다 큽니다. 이런 컬렉션은 `id`라는 필드를 따로 선언할 수 없어서, 선언하면 열 때 `INVALID_ARGUMENT`로 실패합니다. 그 이름의 필드가 필요하면 키로 지정해야 합니다.
- **길이.** 문자열이나 바이트 키는 엔진이 다룰 수 있을 만큼 짧아야 합니다. 키보다 몇 바이트 긴 인코딩이 4096바이트 페이지 파일에서 957바이트 안에 들어가야 하고, 더 긴 키는 객체를 쓸 때 `INVALID_ARGUMENT`로 거부됩니다.

```ts
db.write((txn) => {
  const users = txn.collection('users');

  users.insert({ name: 'Alice' }); // 1
  users.insert({ id: 10, name: 'Bob' }); // 10
  users.insert({ name: 'Carol' }); // 11
  txn.collection('teams').insert({ name: 'north' }); // 'north'
});
```

## 여러 필드에 두는 인덱스

[t](./t.md)에서 `index()`나 `unique()`를 붙인 필드에는 그 필드만의 인덱스가 생깁니다. `options.indexes`로는 여러 필드에 인덱스 하나를 둡니다. 그 항목은 첫 필드로 정렬되고, 값이 같으면 둘째 필드로, 그다음은 셋째 필드로 정렬됩니다. 내장 객체 안의 필드에 인덱스를 둘 때도 경로로 이름을 적어 여기에 선언합니다.

```ts
interface CollectionOptions<F extends Fields = Fields> {
  readonly indexes?: readonly IndexDeclaration<F>[];
}

interface IndexDeclaration<F extends Fields = Fields> {
  readonly fields: readonly [IndexPath<F>, ...IndexPath<F>[]];
  readonly unique?: boolean;
}
```

```ts
const posts = collection(
  {
    author: t.string(),
    written: t.date(),
    place: t.object({ city: t.string() })
  },
  {
    indexes: [{ fields: ['author', 'written'] }, { fields: ['place.city'] }]
  }
);
```

`fields`의 값은 필드 이름이거나, 내장 객체를 따라 이름을 `.`으로 이은 경로입니다. 인덱스 하나에 한 개에서 여덟 개까지 줄 수 있습니다. `unique: true`이면 모든 필드의 값이 같은 객체는 둘일 수 없지만, 어느 한 필드가 null인 객체는 몇 개가 있어도 됩니다. 모양이 다른 옵션은 곧바로 `INVALID_ARGUMENT`를 던집니다. 컬렉션에 없는 필드, 링크를 거치는 경로, 필드가 여럿인데 목록을 거치는 경로처럼 스키마가 가질 수 없는 인덱스는 열 때 `INVALID_ARGUMENT`로 실패합니다.

쿼리가 앞쪽 필드들을 `eq`, `isNull`, `in`으로 같은지 검사하고, 그다음 필드가 있다면 그 필드를 범위나 `startsWith`로 검사하면 이 인덱스를 읽습니다. 같은지 검사한 필드 뒤의 필드를 모두 인덱스의 순서대로 정렬하는 쿼리에는 인덱스가 객체를 정렬된 순서로 내줍니다. 그래서 작성자 한 명의 글을 `written` 내림차순으로 정렬해 20개만 가져오는 쿼리는 항목 스무 개를 읽고 멈춥니다. 이런 인덱스가 있는 파일은 객체 형식 2이며, 이 형식으로 올릴지는 [OpenOptions](../../types/node/open-options.md#upgradeformat)의 `upgradeFormat` 옵션이 정합니다.

돌려받는 `Collection`은 동결된 객체입니다.

```ts
interface Collection<F extends Fields = Fields>
```

## 속성

### fields

```ts
readonly fields: F;
```

선언한 그대로의 필드 타입을 이름별로 담고 있습니다. 자동 `id`는 들어 있지 않습니다.
