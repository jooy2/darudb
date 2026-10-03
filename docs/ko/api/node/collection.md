---
title: collection
order: 3
---

# collection

`collection`은 스키마에 들어갈 컬렉션 하나를 선언하며, 필드를 정하면 기본 키와 인덱스도 함께 정해집니다.

```ts
const collection: <F extends Fields>(fields: F) => Collection<F>;
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
