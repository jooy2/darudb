---
title: 객체 타입
order: 7
counterpart: /types/rust/object
---

# 객체 타입

객체 타입은 컬렉션의 [필드](./field-types.md)에서 TypeScript가 객체의 타입을 정하는 방식으로, 데이터베이스가 돌려주는 객체와 쓸 때 넘기는 객체의 타입을 따로 정합니다.

`ObjectOf`와 `EmbeddedOf`는 읽은 객체의 타입이고, `InsertOf`와 `EmbeddedInputOf`는 쓸 객체의 타입입니다. 패키지가 이 타입들을 알아서 적용하므로, 스키마를 주고 연 `Database`에서 `txn.collection('users')`는 이미 이 타입의 객체를 읽고 씁니다. 객체를 받거나 돌려주는 함수를 직접 만들 때 이 이름을 쓰면 됩니다.

실행할 때 객체는 필드마다 속성이 하나씩 있는 평범한 JavaScript 객체입니다. 데이터베이스에서 읽은 객체 안에는 데이터베이스를 가리키는 것이 없어서, 트랜잭션이 끝난 뒤에도 그대로 쓸 수 있습니다.

## 예시

```ts
import { collection, schema, t } from 'darudb';
import type { InsertOf, ObjectOf } from 'darudb';

const app = schema(1, {
  teams: collection({ name: t.string().primaryKey() }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0),
    visits: t.bigint().default(0n),
    tags: t.list(t.string()).optional(),
    team: t.link('teams').optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
  })
});

type UserFields = typeof app.collections.users.fields;
type User = ObjectOf<UserFields>;
type NewUser = InsertOf<UserFields>;
type Team = ObjectOf<typeof app.collections.teams.fields>;
```

TypeScript는 이 타입들을 다음과 같이 풀어냅니다.

```ts
type User = {
  name: string;
  email: string | null;
  age: number;
  visits: bigint;
  tags: string[] | null;
  team: Key | null;
  address: { city: string; zip: number | null } | null;
  id: number;
};

type NewUser = {
  name: string;
  email?: string | null;
  age?: number | null;
  visits?: bigint | number | null;
  tags?: string[] | null;
  team?: Key | null;
  address?: { city: string; zip?: number | null } | null;
  id?: number;
};

type Team = {
  name: string;
};
```

타입 검사기는 링크를 따라가 대상 컬렉션의 키 필드까지 보지 않으므로, 링크의 타입은 넷 중 어느 것이든 될 수 있는 [Key](./key.md)입니다.

## ObjectOf

```ts
type ObjectOf<F extends Fields> = Simplify<
  EmbeddedOf<F> & (HasKey<F> extends true ? unknown : { id: number })
>;
```

데이터베이스에 든 그대로의 객체입니다. 컬렉션의 필드가 모두 있는데, 비어 있는 선택 필드는 null이고 기본값이 있는 필드에는 언제나 값이 있습니다. 키 필드가 없는 컬렉션이면 엔진이 매긴 키인 `id`가 붙습니다. `get`, `find`, `findOne`이 이 타입의 객체를 돌려줍니다. `Simplify`와 `HasKey`는 내부 도우미입니다. `Simplify`는 교차 타입을 객체 타입 하나로 펴고, `HasKey`는 `primaryKey()`로 표시한 필드가 있는지 알려 줍니다.

## InsertOf

```ts
type InsertOf<F extends Fields> = Simplify<
  EmbeddedInputOf<F> & (HasKey<F> extends true ? unknown : { id?: number })
>;
```

쓸 때 넘기는 객체입니다. 기본값 없는 필수 필드는 반드시 있어야 하고, 나머지는 있어도 없어도 됩니다. `insert`, `insertMany`, `put`, `putMany`가 이 타입의 객체를 받고, `update`는 이 타입의 `Partial`을 받습니다. 키 필드가 없는 컬렉션에서는 `id`를 줘도 되고 빼도 되며, 빼면 다음 번호를 받습니다.

빠졌거나 `undefined`나 `null`인 필드는 값 없이 씁니다. 그러면 선택 필드는 null로, 기본값이 있는 필드는 기본값으로 읽힙니다. 기본값 없는 필수 필드가 빠지면 `INVALID_ARGUMENT`로 실패합니다. 스키마에 없는 속성도 같은 오류로 실패하므로, 철자를 틀린 이름이 아무 말 없이 버려지는 일은 없습니다.

## EmbeddedOf

```ts
type EmbeddedOf<F extends Fields> = Simplify<{ [K in keyof F]: ValueOf<F[K]> }>;
```

읽은 내장 객체의 필드입니다. 필드가 모두 있고, 비어 있는 선택 필드는 null입니다. `t.object(fields)` 필드에 이 타입의 값이 들어 있으며, `id`가 없는 `ObjectOf`와 같습니다.

## EmbeddedInputOf

```ts
type EmbeddedInputOf<F extends Fields> = Simplify<
  { [K in RequiredKeys<F>]: InputOf<F[K]> } & {
    [K in Exclude<keyof F, RequiredKeys<F>>]?: InputOf<F[K]> | null;
  }
>;
```

쓸 때 넘기는 내장 객체의 필드로, `id`가 없는 `InsertOf`와 같습니다. 내장 객체는 통째로 쓰므로, 내장 객체를 줄 때는 `update`에서도 그 필수 필드가 모두 있어야 합니다.

## JavaScript 값

| 필드 타입            | 읽을 때                 | 쓸 때                                  |
| -------------------- | ----------------------- | -------------------------------------- |
| `t.bool()`           | `boolean`               | `boolean`                              |
| `t.int()`            | `number`                | 2^53 안의 정수 `number`                |
| `t.bigint()`         | `bigint`                | 64비트 안의 `bigint`나 정수 `number`   |
| `t.float()`          | `number`                | `number`                               |
| `t.string()`         | `string`                | `string`                               |
| `t.bytes()`          | `Uint8Array`            | `Buffer`를 포함한 `Uint8Array`         |
| `t.link(collection)` | `Key`                   | 대상 컬렉션의 키 타입에 맞는 `Key`     |
| `t.list(element)`    | 원소 값의 배열          | null이 없는 원소 값의 배열             |
| `t.object(fields)`   | `fields`의 `EmbeddedOf` | `fields`의 `EmbeddedInputOf`           |
| 비어 있는 선택 필드  | `null`                  | `null`이나 `undefined`, 또는 속성을 뺌 |

- **정수.** 파일은 모든 정수를 64비트로 저장합니다. `t.int()` 필드는 `Number.MAX_SAFE_INTEGER`를 넘는 값을 쓸 때 거부하고, 다른 프로그램이 저장해 둔 그런 값을 읽으면 `INVALID_ARGUMENT`로 실패합니다. `t.bigint()` 필드는 모든 값을 `bigint`로 읽습니다. 파일에는 정수의 종류가 하나뿐이므로, 스키마 버전을 올리지 않고도 필드를 `t.int()`와 `t.bigint()` 사이에서 바꿀 수 있습니다.
- **부동소수점 수** 필드는 어떤 number든 받습니다. 정수 필드는 소수부가 있는 수를 받지 않아서, `1.5`는 `INVALID_ARGUMENT`로 실패합니다.
- **문자열**은 UTF-8로 나타낼 수 있어야 합니다. `'\ud800'`처럼 짝이 없는 서로게이트가 든 문자열은 `INVALID_ARGUMENT`로 실패합니다.
- **바이트**는 읽을 때마다 새 `Uint8Array`로 돌아옵니다.
- **링크**에는 [Key](./key.md)에서 설명한 대로 대상 객체의 키가 들어갑니다. number나 2^53을 넘으면 `bigint`, 문자열, `Uint8Array`입니다. 대상 컬렉션의 키와 타입이 다른 값은 `INVALID_ARGUMENT`로 실패합니다.
- **리스트**에는 null이 들어갈 수 없고, null인 원소는 `INVALID_ARGUMENT`로 실패합니다. 빈 리스트는 null이 아닙니다.
- **내장 객체**는 자기 필드를 모두 갖춘 채로 읽히고, 비어 있는 필드는 null이나 기본값이 됩니다.
