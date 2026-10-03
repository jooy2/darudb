---
title: Key
order: 5
---

# Key

`Key` is a primary key as JavaScript passes and receives it: a number or a `bigint` for an int key, a string, or bytes.

```ts
type Key = number | bigint | string | Uint8Array;
```

`get`, `update` and `delete` take a key, `insert`, `insertMany`, `put` and `putMany` return the keys of the objects they write, and `previousKeys` returns keys too. A link field holds the key of the object it links to. Which of the four a collection's keys are follows from its key field, which [`primaryKey()`](./field-types.md#keyabletype) marks:

| Key field                 | Passed as                  | Returned as                       |
| ------------------------- | -------------------------- | --------------------------------- |
| None, the engine's `id`   | `number` or `bigint`       | `number`                          |
| `t.int().primaryKey()`    | `number` or `bigint`       | `number`                          |
| `t.bigint().primaryKey()` | `bigint` or `number`       | `number`, or `bigint` beyond 2^53 |
| `t.string().primaryKey()` | `string`                   | `string`                          |
| `t.bytes().primaryKey()`  | `Uint8Array`, `Buffer` too | `Buffer`                          |

- **Ints.** An int key is passed as a whole number within `Number.MAX_SAFE_INTEGER`, or as a `bigint` within 64 bits, whatever its field's type: `get(2n ** 40n)` finds the object whose key is `2 ** 40`. A number with a fraction, or a number beyond that range, fails with `INVALID_ARGUMENT`. A key comes back as a number when a number holds it exactly and as a `bigint` beyond that, which only a `t.bigint()` key field can hold: a `t.int()` field refuses such a value when it is written. An object read back holds its `t.bigint()` key field as a `bigint` either way, even when `insert` returned the key as a number.
- **Strings** are passed and returned as strings.
- **Bytes** are passed as any `Uint8Array`, a `Buffer` included. `insert` and the other writes return them as a `Buffer`, which is a `Uint8Array`, and an object read back holds them as a plain `Uint8Array` of its own.

A key of another type than the collection's key field fails with `INVALID_ARGUMENT`, and so does `null`, a boolean or any other value. A string or bytes key also has to fit in a key of the file, which is at most about a quarter of a page: its encoding, the bytes plus three with each zero byte taking two, may be up to 957 bytes with 4096-byte pages. A longer key fails with `INVALID_ARGUMENT`.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  files: collection({ hash: t.bytes().primaryKey(), size: t.int() }),
  counters: collection({ n: t.bigint().primaryKey() })
});
const db = Database.open('files.darudb', { schema: app });

db.write((txn) => {
  const files = txn.collection('files');
  const key = files.insert({ hash: new Uint8Array([0xca, 0xfe]), size: 2 }); // a Buffer

  files.get(key);

  const counters = txn.collection('counters');

  counters.insert({ n: 2n ** 60n }); // 1152921504606846976n
  counters.insert({ n: 5n }); // 5, a number
});
```
