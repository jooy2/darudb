---
title: Field types
order: 6
---

# Field types

The field types are what [`t`](../../api/node/t.md) makes for the fields of a schema, each with the modifiers that field can have.

A field type describes a field to the engine, and to TypeScript through type parameters that exist only for the type checker: `T` is the value an object read from the database holds, `M` the field's [mode](#fieldmode), and `I` the value written. Every modifier returns a new type and leaves the one it was called on as it was, so one type can start several fields. A type lacks the modifiers the engine would refuse. Called from JavaScript anyway, most of them fail with `INVALID_ARGUMENT` at once, and the rest when the database opens.

| Builder              | Type                                                              |
| -------------------- | ----------------------------------------------------------------- |
| `t.bool()`           | `FieldType<boolean>`                                              |
| `t.int()`            | `KeyableType<number>`                                             |
| `t.bigint()`         | `KeyableType<bigint, bigint \| number>`                           |
| `t.float()`          | `FieldType<number>`                                               |
| `t.string()`         | `KeyableType<string>`                                             |
| `t.bytes()`          | `KeyableType<Uint8Array>`                                         |
| `t.link(collection)` | `LinkType`                                                        |
| `t.list(element)`    | `FieldType<T[], 'required', I[]>`, or `FieldType<Key[]>` of links |
| `t.object(fields)`   | `EmbeddedType<EmbeddedOf<F>, EmbeddedInputOf<F>>`                 |

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

[Object types](./object-types.md) shows the object types TypeScript derives from fields like these.

## FieldMode

```ts
type FieldMode = 'required' | 'optional' | 'default';
```

How a field holds its value. Every type starts as `'required'`.

- **`'required'`**: every object written has the field, and it is never null.
- **`'optional'`**: the field may be null, and is null when it is left out. `optional()` makes it so.
- **`'default'`**: the field is required, and holds its default when it is left out. `default(value)` makes it so.

## FieldType

```ts
interface FieldType<T, M extends FieldMode = 'required', I = T> extends Typed<T, M, false, I> {
  optional(): FieldType<T, 'optional', I>;
  default(value: I): FieldType<T, 'default', I>;
  index(): FieldType<T, M, I>;
  unique(): FieldType<T, M, I>;
}
```

The type of an ordinary field. `t.bool()`, `t.float()` and `t.list(element)` make one, and so do the modifiers of a [KeyableType](#keyabletype).

- **`optional()`**: the field may be null, and is null when it is left out.
- **`default(value)`**: the field holds `value` when it is left out. A value that does not have the field's type fails with `INVALID_ARGUMENT` when the database opens.
- **`index()`**: the engine keeps an index on the field, so that a query on it reads the index rather than every object. An index on a list has an entry for each element.
- **`unique()`**: an index that also refuses a second object with the same value, with `DUPLICATE_KEY`. Any number of objects may hold null.

A list's element is a type without modifiers: an optional or defaulted element fails with `INVALID_ARGUMENT` when the database opens, and so does a list of lists.

## KeyableType

```ts
interface KeyableType<T, I = T> extends FieldType<T, 'required', I> {
  primaryKey(): KeyType<T, I>;
}
```

The type of a field that can be the primary key: `t.int()`, `t.bigint()`, `t.string()` and `t.bytes()` make one. It has every modifier of `FieldType`, and `primaryKey()`, which makes the field the collection's primary key. Call `primaryKey()` before any other modifier: they return a `FieldType`, which has no `primaryKey()`.

A collection has at most one key field, and two fail with `INVALID_ARGUMENT` when the database opens. A collection without one gets an `id` that the engine numbers from 1, as [`collection`](../../api/node/collection.md) explains.

## KeyType

```ts
interface KeyType<T, I = T> extends Typed<T, 'required', true, I> {
  index(): KeyType<T, I>;
  unique(): KeyType<T, I>;
}
```

The primary key's field, as `primaryKey()` makes it: required, without a default, and never optional. Its third type parameter, `true`, is how [ObjectOf](./object-types.md#objectof) knows that the collection's objects have no engine-numbered `id`. Of the modifiers, it keeps `index()` and `unique()`.

## LinkType

```ts
interface LinkType<M extends FieldMode = 'required'> extends Typed<Key, M, false, Key> {
  optional(): LinkType<'optional'>;
  index(): LinkType<M>;
  unique(): LinkType<M>;
}
```

A link: the primary key of an object in the collection that `t.link(collection)` names, which may be the collection the link is in. It is read and written as a [Key](./key.md) of that collection's key type. It has `optional()`, `index()` and `unique()`, and no default, which would name an object. A link to an object that does not exist is allowed, and a link to a collection the schema does not have fails with `INVALID_ARGUMENT` when the database opens. `t.list(t.link(collection))` is a to-many link.

## EmbeddedType

```ts
interface EmbeddedType<T, I, M extends FieldMode = 'required'> extends Typed<T, M, false, I> {
  optional(): EmbeddedType<T, I, 'optional'>;
}
```

An embedded object, with fields of its own, which `t.object(fields)` makes. It has no primary key and no collection, and is read and written whole with the object that holds it. Its only modifier is `optional()`: it has no default, since its fields have their own, and no index. A field inside it cannot be indexed or be a key either, which fails with `INVALID_ARGUMENT` when the database opens, and a list cannot hold embedded objects. A query reaches its fields with a path such as `address.city`.

`T` is [EmbeddedOf](./object-types.md#embeddedof) of its fields, and `I` is [EmbeddedInputOf](./object-types.md#embeddedinputof).

## AnyField

```ts
type AnyField = Typed<any, FieldMode, boolean, any>;
```

Any field's type, for code that handles fields of every kind. `Typed<T, M, K, I>` is the interface every field type extends, where `K` says whether the field is the primary key. Its four members exist only for the type checker: no field type has them at run time.

## Fields

```ts
type Fields = Record<string, AnyField>;
```

The fields of a collection or an embedded object, by name, as [`collection`](../../api/node/collection.md) and `t.object` take them. The object types take a `Fields` as their type parameter.
