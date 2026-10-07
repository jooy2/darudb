---
title: ReadCollection
order: 7
counterpart: /api/rust/collection-reader
---

# ReadCollection

`ReadCollection`은 트랜잭션 안에서 컬렉션 하나의 객체를 기본 키나 쿼리로 읽습니다.

```ts
interface ReadCollection<O>
```

읽기 트랜잭션의 `collection`이 돌려주며, 쓰기 트랜잭션의 [WriteCollection](./write-collection.md)에도 아래 멤버가 모두 있어서 트랜잭션 자신의 변경까지 읽습니다. `O`는 컬렉션의 객체 타입이고, [ObjectOf](../../types/node/object-types.md)가 필드로 만듭니다. 메서드는 트랜잭션의 함수가 도는 동안만 부를 수 있고, 그 뒤에는 `CLOSED`를 던집니다.

읽어 온 객체는 트랜잭션이 끝나도 남는 평범한 객체입니다. 스키마의 필드가 모두 들어 있고, 쓸 때 빠진 필드에는 기본값이나 null이 들어갑니다. 키 필드가 없는 컬렉션의 객체에는 `id`가 있습니다. `t.bigint()`로 선언한 필드는 `bigint`로, 바이트는 `Uint8Array`로 읽힙니다. `t.int()`로 선언한 필드에 2^53 너머의 값이 있으면 읽을 때 `INVALID_ARGUMENT`로 실패합니다.

## 속성

### name

```ts
readonly name: string;
```

컬렉션의 이름입니다.

## 메서드

### get

```ts
get(key: Key): O | null;
```

기본 키가 `key`인 객체이고, 없으면 `null`입니다. [Key](../../types/node/key.md)는 number나 `bigint`로 쓴 정수, 문자열, 바이트입니다. `bigint`는 값이 같은 정수를 찾습니다. `1.5`, `true`, `null`처럼 그 밖의 값을 주면 `INVALID_ARGUMENT`로 실패합니다.

### find

```ts
find(query?: QueryInput<O>): O[];
find(text: string, parameters?: QueryParameters): O[];
find(prepared: Prepared<O>, parameters?: QueryParameters): O[];
```

쿼리가 찾은 객체를 쿼리의 순서대로 돌려줍니다. 쿼리는 [QueryInput](../../types/node/query-input.md)에 선언된 대로 다음 중 하나로 줍니다.

- **주지 않으면** 모든 객체를 기본 키 순서로 돌려줍니다.
- **함수**는 새 [Query](./query.md)를 받아 조건을 더합니다. 쿼리를 돌려줘도 되고 아무것도 돌려주지 않아도 됩니다.
- **`Query`는** `new Query()`로 미리 만든 쿼리입니다.
- **문자열**은 [쿼리 언어](./query.md#쿼리-언어)로 쓴 쿼리입니다. `$0`, `$1` 같은 매개변수에는 `parameters`의 값이 순서대로 들어갑니다. 패키지는 4096자 이하의 문자열을 256개까지 해석해 둔 채로 기억하므로, 같은 문자열을 다시 실행하면 해석을 건너뜁니다.
- **준비한 쿼리**는 `Database.prepare`로 이 컬렉션에 준비한 쿼리이며, 매개변수 값과 함께 줍니다.

쿼리가 맞지 않으면 `INVALID_QUERY`로 실패합니다. 컬렉션에 없는 필드, 타입이 다른 값, 해석할 수 없는 문자열, 단일 값의 배열이 아닌 매개변수, 다른 컬렉션에 준비한 쿼리, 값을 받지 못한 매개변수가 그런 경우입니다.

```ts
import { Query } from 'darudb';
import type { ObjectOf } from 'darudb';

type User = ObjectOf<typeof app.collections.users.fields>;

const adults = new Query<User>().where('age', '>=', 18);

db.read((txn) => {
  const users = txn.collection('users');

  users.find();
  users.find((q) => q.where('tags', 'contains', 'new').sortBy('age', 'desc').limit(10));
  users.find(adults);
  users.find('age >= $0 AND name STARTSWITH $1', [18, 'A']);
});
```

### findOne

```ts
findOne(query?: QueryInput<O>): O | null;
findOne(text: string, parameters?: QueryParameters): O | null;
findOne(prepared: Prepared<O>, parameters?: QueryParameters): O | null;
```

쿼리가 찾은 첫 객체이고, 없으면 `null`입니다. 쿼리는 `find`와 같은 형태로 받으며, 엔진은 첫 객체에서 읽기를 멈춥니다. 쿼리에 준 오프셋과 개수 제한은 그대로 적용되므로, 개수 제한이 0이면 아무것도 찾지 않습니다.

### count

```ts
count(query?: QueryInput<O>): number;
count(text: string, parameters?: QueryParameters): number;
count(prepared: Prepared<O>, parameters?: QueryParameters): number;
```

쿼리가 찾은 객체의 수입니다. 오프셋을 건너뛰고 개수 제한 안에서 셉니다. 쿼리를 주지 않으면 모든 객체를 세는데, 이때는 객체를 읽지 않고 컬렉션이 기록해 둔 개수만 읽습니다.

## AsyncReadCollection

```ts
interface AsyncReadCollection<O> {
  readonly name: string;
  get(key: Key): Promise<O | null>;
  find(query?: QueryInput<O>): Promise<O[]>;
  find(text: string, parameters?: QueryParameters): Promise<O[]>;
  find(prepared: Prepared<O>, parameters?: QueryParameters): Promise<O[]>;
  findOne(query?: QueryInput<O>): Promise<O | null>;
  findOne(text: string, parameters?: QueryParameters): Promise<O | null>;
  findOne(prepared: Prepared<O>, parameters?: QueryParameters): Promise<O | null>;
  count(query?: QueryInput<O>): Promise<number>;
  count(text: string, parameters?: QueryParameters): Promise<number>;
  count(prepared: Prepared<O>, parameters?: QueryParameters): Promise<number>;
}
```

비동기 트랜잭션의 컬렉션입니다. 멤버가 하는 일은 위와 같고 결과를 promise로 돌려줍니다. 실패하면 같은 코드로 promise가 거부되며, `CLOSED`도 마찬가지입니다. 작업은 스레드 풀에서 부른 순서대로 하나씩 실행됩니다. 이벤트 루프의 한 턴 안에서 부른 작업과, 앞선 묶음이 스레드 풀에 가 있는 동안 부른 작업은 한 묶음으로 엔진에 갑니다. 그래서 여러 작업을 한꺼번에 시작하고 함께 기다리는 쪽이 하나씩 await하는 것보다 훨씬 쌉니다.

```ts
const found = await db.readAsync((txn) => {
  const users = txn.collection('users');

  return Promise.all([1, 2, 3].map((key) => users.get(key)));
});
```
