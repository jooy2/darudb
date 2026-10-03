---
title: t
order: 4
counterpart: /types/rust/type
---

# t

`t` holds the builders of field types, with which `collection` and `t.object` declare their fields.

```ts
const t: TypeBuilders;
```

Each builder returns a new field type, and each of its modifiers returns a copy, so one type can be used for any number of fields. The type a builder returns, which [Field types](../../types/node/field-types.md) describes, carries the field's value for the type checker: an object read from the database holds that value, and an object written gives it.

A modifier that a type cannot have is refused twice: by TypeScript, since the returned type does not have it, and at run time with `INVALID_ARGUMENT`, when the field is declared or when the database opens.

```ts
import { collection, t } from 'darudb';

const users = collection({
  name: t.string(),
  email: t.string().optional().unique(),
  age: t.int().default(0).index(),
  tags: t.list(t.string()).optional().index(),
  team: t.link('teams').optional(),
  address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
});
```

## Methods

### bool

```ts
bool(): FieldType<boolean>;
```

`true` or `false`.

### int

```ts
int(): KeyableType<number>;
```

A 64-bit int, read as a number. A value beyond 2^53, which a number does not hold exactly, is refused when it is written and fails when it is read, both with `INVALID_ARGUMENT`; declare a field that needs such values with `bigint`. A number with a fraction is refused.

### bigint

```ts
bigint(): KeyableType<bigint, bigint | number>;
```

A 64-bit int, read as a `bigint` whatever its size, and written as a `bigint` or a number. The file stores one kind of int, so declaring a field with `bigint` where it was declared with `int`, or the other way round, changes only how it is read, and is not a change of schema.

### float

```ts
float(): FieldType<number>;
```

A 64-bit floating-point number. `-0` equals `0`, and every NaN equals every other and sorts after positive infinity, so that an index can keep the field in order.

### string

```ts
string(): KeyableType<string>;
```

Text, stored as UTF-8. A string that UTF-8 cannot hold, such as one with a lone surrogate, is refused with `INVALID_ARGUMENT`. Strings compare and sort by their UTF-8 bytes, with no collation and no case folding.

### bytes

```ts
bytes(): KeyableType<Uint8Array>;
```

Any bytes, written and read as a `Uint8Array`. A `Buffer` is one.

### link

```ts
link(collection: string): LinkType;
```

The primary key of an object of collection `collection`, which has to be in the same schema, or the database fails to open with `INVALID_ARGUMENT`. The field holds the key and nothing else, and it may name an object that does not exist. A query path through it reads the linked object, as in `team.city`. A link has no default.

### list

```ts
list<T, I>(
  element: FieldType<T, 'required', I> | KeyableType<T, I>
): FieldType<T[], 'required', I[]>;
list(element: LinkType): FieldType<Key[]>;
```

A list of values of `element`, a scalar type or a link without modifiers. A list of links is a to-many link. A list holds no nulls, and an empty list is not null. An element type that is optional or has a default, or a list of lists or of embedded objects, fails to open with `INVALID_ARGUMENT`. An index on a list has an entry for each element, and a condition on a list holds when it holds for any element.

### object

```ts
object<F extends Fields>(fields: F): EmbeddedType<EmbeddedOf<F>, EmbeddedInputOf<F>>;
```

An embedded object with fields of its own, stored inside the object that holds it and read and written with it. It has no key, no default and no index. Its fields follow their own types and modifiers, except that none of them can be a key or indexed, and a query reaches them with a path such as `address.city`. A value of `fields` that is not a type from `t` throws `INVALID_ARGUMENT` at once.

## Modifiers

The modifiers are methods of the field types above. Which ones a type has is part of its type: [Field types](../../types/node/field-types.md) lists them.

### optional

```ts
optional(): FieldType<T, 'optional', I>;
```

The field may be null, and is null when it is left out. A primary key cannot be optional.

### default

```ts
default(value: I): FieldType<T, 'default', I>;
```

The field is required, and holds `value` when it is left out. The default is written into the object, so a later change of the default leaves written objects as they are; only objects written before the field existed read the new one. A later schema version can change a default, but cannot take it away while the field stays required. A link, an embedded object and a primary key have no default.

### index

```ts
index(): FieldType<T, M, I>;
```

Queries on the field read an index rather than every object. A field inside an embedded object cannot be indexed.

### unique

```ts
unique(): FieldType<T, M, I>;
```

An index that also refuses two objects with the same value, with `DUPLICATE_KEY`. Any number of objects may hold null. `unique` makes the index by itself, without `index`.

### primaryKey

```ts
primaryKey(): KeyType<T, I>;
```

The field is the collection's primary key. Only `int`, `bigint`, `string` and `bytes` return a type that has it, and the other modifiers return types without it. The key's type has only `index` and `unique` left. [collection](./collection.md#primary-keys) explains primary keys.
