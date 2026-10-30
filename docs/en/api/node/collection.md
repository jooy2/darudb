---
title: collection
order: 3
---

# collection

`collection` declares one collection of a schema: its fields, by name, and through them its primary key and indexes.

```ts
const collection: <F extends Fields>(fields: F, options?: CollectionOptions<F>) => Collection<F>;
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

## Indexes on several fields

A field marked `index()` or `unique()` in [t](./t.md) has an index of its own. `options.indexes` declares an index on several fields, whose entries sort by the first field, then by the second, and so on, or one on a field inside an embedded object, named by its path:

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

Each of `fields` is a field's name, or names joined by `.` through embedded objects, and an index has one to eight of them. With `unique: true`, two objects cannot hold the same values in all of them, though any number may hold null in one. Options of another shape throw `INVALID_ARGUMENT` at once; an index the schema cannot have, on a field it does not have, through a link, or on several fields with a path through a list, fails to open with `INVALID_ARGUMENT`.

A query reads the index when it tests the first fields for equality, with `eq`, `isNull` or `in`, and the next one, if any, for a range or with `startsWith`. The index gives its objects in order when the query sorts by every field after those it tests for equality, in the index's order: a query for one author's posts sorted by `written` descending, with a limit of 20, reads twenty entries and stops. A file with such an index is in object format 2, which the `upgradeFormat` option of [OpenOptions](../../types/node/open-options.md#upgradeformat) governs.

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
