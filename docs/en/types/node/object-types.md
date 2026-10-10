---
title: Object types
order: 7
group: objects
counterpart: /types/rust/object
pageClass: reference-page
---

# Object types

The object types say how TypeScript types an object of a collection, as the database gives it and as it is written, from the collection's [fields](./field-types.md).

`ObjectOf` and `EmbeddedOf` type an object as it is read, and `InsertOf` and `EmbeddedInputOf` an object as it is written. The package applies them for you: in a `Database` opened with a schema, `txn.collection('users')` reads and writes objects of these types. Name them in your own code where a function takes or returns objects.

At run time an object is a plain JavaScript object, with a property for each field. An object read from the database outlives its transaction, since nothing in it refers back to the database.

## Example

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

TypeScript works these out to:

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

A link is typed as any [Key](./key.md), since the type checker does not follow it to the linked collection's key field.

## ObjectOf

```ts
type ObjectOf<F extends Fields> = Simplify<
  EmbeddedOf<F> & (HasKey<F> extends true ? unknown : { id: number })
>;
```

An object as the database holds it. It has every field of the collection: an optional field that is empty holds null, and a field with a default always holds a value. A collection without a key field adds `id`, the key the engine numbered. `get`, `find` and `findOne` return objects of this type. `Simplify` and `HasKey` are internal helpers: the first flattens the intersection into one object type, and the second says whether a field is marked `primaryKey()`.

## InsertOf

```ts
type InsertOf<F extends Fields> = Simplify<
  EmbeddedInputOf<F> & (HasKey<F> extends true ? unknown : { id?: number })
>;
```

An object as it is written. It has the required fields that have no default, and may have any of the rest. `insert`, `insertMany`, `put` and `putMany` take objects of this type, and `update` takes a `Partial` of it. In a collection without a key field, `id` may be given or left out, and an object without one gets the next number.

A field that is left out, `undefined` or `null` is written without a value: an optional field then reads null and a field with a default reads its default. A required field without a default fails with `INVALID_ARGUMENT`, and so does a property the schema does not have, so that a misspelt name is never dropped without a word.

## EmbeddedOf

```ts
type EmbeddedOf<F extends Fields> = Simplify<{ [K in keyof F]: ValueOf<F[K]> }>;
```

The fields of an embedded object as it is read: every one, null where optional and empty. It is what a `t.object(fields)` field holds, and `ObjectOf` without the `id`.

## EmbeddedInputOf

```ts
type EmbeddedInputOf<F extends Fields> = Simplify<
  { [K in RequiredKeys<F>]: InputOf<F[K]> } & {
    [K in Exclude<keyof F, RequiredKeys<F>>]?: InputOf<F[K]> | null;
  }
>;
```

The fields of an embedded object as it is written, which is `InsertOf` without the `id`. An embedded object is written whole, so its required fields are needed whenever the object is given, an `update` included.

## JavaScript values

| Field type | Read as | Written as |
| --- | --- | --- |
| `t.bool()` | `boolean` | `boolean` |
| `t.int()` | `number` | A whole `number` within 2^53 |
| `t.bigint()` | `bigint` | `bigint`, or a whole `number`, within 64 bits |
| `t.float()` | `number` | `number` |
| `t.string()` | `string` | `string` |
| `t.bytes()` | `Uint8Array` | `Uint8Array`, a `Buffer` included |
| `t.link(collection)` | `Key` | `Key` of the linked collection's key type |
| `t.list(element)` | An array of the element's values | An array of the element's values, without nulls |
| `t.object(fields)` | `EmbeddedOf` of `fields` | `EmbeddedInputOf` of `fields` |
| Optional and empty | `null` | `null` or `undefined`, or the property left out |

- **Ints.** The file stores every int in 64 bits. A `t.int()` field refuses a value beyond `Number.MAX_SAFE_INTEGER` when it is written, and reading such a value, which another program may have stored, fails with `INVALID_ARGUMENT`. A `t.bigint()` field reads every value as a `bigint`. Since the file stores one kind of int, a field can change between `t.int()` and `t.bigint()` without a new schema version.
- **Floats** take any number. An int field never takes a fraction: `1.5` fails with `INVALID_ARGUMENT`.
- **Strings** have to be text that UTF-8 can hold. A string with an unpaired surrogate, such as `'\ud800'`, fails with `INVALID_ARGUMENT`.
- **Bytes** read back are a new `Uint8Array` each time.
- **Links** hold the linked object's key as [Key](./key.md) describes it: a number, or a `bigint` beyond 2^53, a string or a `Uint8Array`. A value of another type than the linked collection's key fails with `INVALID_ARGUMENT`.
- **Lists** hold no nulls, and a null element fails with `INVALID_ARGUMENT`. An empty list is not null.
- **Embedded objects** read back with every field of their own, null or the default where one is empty.
