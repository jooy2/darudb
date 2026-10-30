---
title: StoredSchema
order: 18
group: objects
pageClass: reference-page
---

# StoredSchema

`StoredSchema` is a schema in plain data, as [`Database.storedSchema`](../../api/node/database.md#storedschema) gives it.

```ts
interface StoredSchema {
  readonly version: number;
  readonly collections: readonly StoredCollection[];
}
```

It says what a declaration with [`schema`](../../api/node/schema.md), [`collection`](../../api/node/collection.md) and [`t`](../../api/node/t.md) says, without the ids the file adds: the version, and each collection with its fields, its primary key and its indexes. It describes the schema a database was opened with, or, for a database opened without one, the schema the file holds, which a program that opens a file it did not write learns its collections from. Every part of it is frozen.

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

A collection. `key` names the primary key's field, `id` for the auto-increment, and `autoIncrement` says whether the key is the auto-increment, an int `id` the engine assigns. `fields` are in the file's order, and the primary key's field is one of them unless the key is the auto-increment, whose `id` is not declared.

## StoredField

```ts
interface StoredField {
  readonly name: string;
  readonly type: StoredType;
  readonly optional: boolean;
  readonly default?: unknown;
}
```

A field of a collection or of an embedded object. `optional` says whether it may be null, which it is when an object leaves it out. `default` is the value an object that leaves a required field out holds, and is absent when the field has none.

## StoredIndex

```ts
interface StoredIndex {
  readonly field: string;
  readonly fields: readonly string[];
  readonly unique: boolean;
}
```

An index of a collection: the fields it is on, in order, each a name or a path such as `address.city`, and whether two objects may not hold the same values in all of them. `field` is the first of `fields`, the only one of an index declared with `index()` or `unique()`.

## StoredType

```ts
type StoredType =
  | { readonly kind: 'bool' | 'int' | 'float' | 'string' | 'bytes' }
  | { readonly kind: 'link'; readonly collection: string }
  | { readonly kind: 'list'; readonly element: StoredType }
  | { readonly kind: 'object'; readonly fields: readonly StoredField[] };
```

A field's type, which `kind` names. A link names the collection whose primary key it holds, a list the type of its elements, a scalar type or a link, and an embedded object its fields. An int field is `int` whether it was declared with `t.int()` or `t.bigint()`, since the file stores one type of int, so a database opened without a schema reads its values as numbers up to 2^53 and as `bigint`s beyond.
