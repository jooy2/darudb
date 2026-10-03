---
title: Key
order: 5
---

# Key

`Key`는 JavaScript가 주고받는 기본 키로, 정수 키는 number나 `bigint`, 문자열 키는 문자열, 바이트 키는 바이트입니다.

```ts
type Key = number | bigint | string | Uint8Array;
```

`get`, `update`, `delete`가 키를 받고, `insert`, `insertMany`, `put`, `putMany`는 쓴 객체의 키를 돌려주며, `previousKeys`도 키를 돌려줍니다. 링크 필드에는 가리키는 객체의 키가 들어갑니다. 컬렉션의 키가 넷 중 무엇인지는 [`primaryKey()`](./field-types.md#keyabletype)로 정한 키 필드가 정합니다.

| 키 필드                   | 넘길 때                     | 돌려받을 때                      |
| ------------------------- | --------------------------- | -------------------------------- |
| 없음, 엔진의 `id`         | `number`나 `bigint`         | `number`                         |
| `t.int().primaryKey()`    | `number`나 `bigint`         | `number`                         |
| `t.bigint().primaryKey()` | `bigint`나 `number`         | `number`, 2^53을 넘으면 `bigint` |
| `t.string().primaryKey()` | `string`                    | `string`                         |
| `t.bytes().primaryKey()`  | `Uint8Array`, `Buffer`도 됨 | `Buffer`                         |

- **정수.** 정수 키는 필드의 타입과 관계없이 `Number.MAX_SAFE_INTEGER` 안의 정수나 64비트 안의 `bigint`로 넘깁니다. `get(2n ** 40n)`은 키가 `2 ** 40`인 객체를 찾습니다. 소수부가 있는 수나 그 범위를 넘는 수는 `INVALID_ARGUMENT`로 실패합니다. 돌려받는 키는 number가 정확히 담을 수 있으면 number이고, 그보다 크면 `bigint`입니다. 그렇게 큰 값은 `t.bigint()` 키 필드에만 들어갑니다. `t.int()` 필드는 쓸 때 그런 값을 거부합니다. 다시 읽은 객체의 `t.bigint()` 키 필드는 언제나 `bigint`입니다. `insert`가 키를 number로 돌려줬더라도 마찬가지입니다.
- **문자열**은 문자열로 넘기고 문자열로 돌려받습니다.
- **바이트**는 `Buffer`를 포함해 어떤 `Uint8Array`로든 넘깁니다. `insert` 같은 쓰기는 `Uint8Array`의 하나인 `Buffer`로 돌려주고, 다시 읽은 객체에는 따로 복사한 평범한 `Uint8Array`가 들어 있습니다.

컬렉션의 키 필드와 타입이 다른 키는 `INVALID_ARGUMENT`로 실패하고, `null`이나 불리언 같은 값도 마찬가지입니다. 문자열 키와 바이트 키는 파일의 키에 들어가야 하는데, 파일의 키는 길어야 페이지의 4분의 1쯤입니다. 키를 인코딩하면 바이트에 3바이트가 붙고 0인 바이트는 2바이트를 차지하는데, 4096바이트 페이지에서는 이 인코딩이 957바이트까지 될 수 있습니다. 더 긴 키는 `INVALID_ARGUMENT`로 실패합니다.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  files: collection({ hash: t.bytes().primaryKey(), size: t.int() }),
  counters: collection({ n: t.bigint().primaryKey() })
});
const db = Database.open('files.darudb', { schema: app });

db.write((txn) => {
  const files = txn.collection('files');
  const key = files.insert({ hash: new Uint8Array([0xca, 0xfe]), size: 2 }); // Buffer

  files.get(key);

  const counters = txn.collection('counters');

  counters.insert({ n: 2n ** 60n }); // 1152921504606846976n
  counters.insert({ n: 5n }); // 5, number
});
```
