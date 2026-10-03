---
title: collection
order: 3
---

# collection

`collection` declares one collection of a schema: its fields, by name, and through them its primary key and indexes.

```ts
const collection: <F extends Fields>(fields: F) => Collection<F>;
```

Each value of `fields` is a field type from [t](./t.md); anything else throws `INVALID_ARGUMENT` at once. The collection gets its name from [schema](./schema.md), which takes it under that name. The types of its objects follow from the fields: [ObjectOf](../../types/node/object-types.md) is an object as it is read, and `InsertOf` an object as it is written.

```ts
import { collection, t } from 'darudb';

const teams = collection({ name: t.string().primaryKey(), city: t.string().optional() });
const users = collection({ name: t.string(), team: t.link('teams').optional() });
```

## Primary keys

Every object has a primary key, which `get`, `update` and `delete` take and which no two objects of a collection share.

- **A key field.** A field marked `primaryKey()`, of type `t.int()`, `t.bigint()`, `t.string()` or `t.bytes()`, is the key. It is required and has no default, and an object's key never changes: writing an object under another key writes another object. A collection with two such fields fails to open with `INVALID_ARGUMENT`.
- **The automatic `id`.** A collection without a key field gets an int field called `id`, read as a number. An object written without an `id` gets the next number, from 1 up, and a number is never given twice in one file, even after its object is deleted. An object written with an `id` of its own keeps it, and the numbers given later are greater than it. Such a collection cannot declare a field called `id` itself: opening it fails with `INVALID_ARGUMENT`, and a field of that name has to be the key.
- **Length.** A string or bytes key has to be short enough for the engine: its encoding, a few bytes longer than the key, has to fit in 957 bytes in a file of 4096-byte pages. A longer key is refused with `INVALID_ARGUMENT` when the object is written.

```ts
db.write((txn) => {
  const users = txn.collection('users');

  users.insert({ name: 'Alice' }); // 1
  users.insert({ id: 10, name: 'Bob' }); // 10
  users.insert({ name: 'Carol' }); // 11
  txn.collection('teams').insert({ name: 'north' }); // 'north'
});
```

The returned `Collection` is frozen:

```ts
interface Collection<F extends Fields = Fields>
```

## Properties

### fields

```ts
readonly fields: F;
```

The field types, by name, as they were declared. The automatic `id` is not among them.
