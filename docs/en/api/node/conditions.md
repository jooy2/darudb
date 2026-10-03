---
title: conditions
order: 10
counterpart: /api/rust/filter
---

# conditions

`conditions` makes the conditions that a query's filter is made of, and joins them with `and`, `or` and `not`.

```ts
const conditions: Conditions;
```

`Query.where` takes a condition, or a function that receives the same conditions typed for the collection: `q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email')))`. In the function, `c` is a `Conditions<O>` whose methods accept only the collection's fields and values of their types. The exported `conditions` is a `Conditions` of no particular collection, for building a condition outside a query: it accepts any field and any value, and the engine checks them when the query runs.

Every method returns a `Condition`:

```ts
interface Condition
```

A condition is frozen and can go into any number of queries. Its one property, `node`, is the package's own and not for use. Anything other than a condition where one is expected fails with `INVALID_QUERY`.

The signatures use the helper types that [Query](./query.md) describes: `FieldNames<O>`, `DottedPath`, `ElementOf<T>` and `Operand<T>`.

- **Paths.** A field is named, or reached through embedded objects and links with `.`: `address.city`, or `team.city` to test the linked object. A link to an object that is not there reads as null. A path has at most 32 names, and TypeScript does not check a dotted one.
- **Lists.** A condition on a list holds when it holds for any element, and `contains` on a list looks for an element. An empty list has no element, so only `isNotNull` holds for it.
- **Null.** Every condition on a null field is false, except `isNull`. `eq(field, null)` is `isNull`, and `ne(field, null)` is `isNotNull`.
- **Types.** A value has the field's type. An int field compares with an int and never a float, a float field with any number, and a link with the linked collection's key. A query that breaks this, or names a field the collection does not have, fails with `INVALID_QUERY`.
- **Nesting.** A filter nests at most 24 levels deep. An `and` inside an `and`, and an `or` inside an `or`, become one level, so only `not` and alternating groups count.

```ts
import { conditions } from 'darudb';

const young = conditions.and(conditions.ge('age', 18), conditions.lt('age', 30));
const found = db.read((txn) =>
  txn.collection('users').find((q) => q.where(conditions.or(young, conditions.isNull('email'))))
);
```

## Methods

### eq

```ts
eq<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]> | null>): Condition;
eq(path: DottedPath, value: Operand<QueryValue | null>): Condition;
```

The field equals `value`. With `null`, the field is null.

### ne

```ts
ne<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]> | null>): Condition;
ne(path: DottedPath, value: Operand<QueryValue | null>): Condition;
```

The field differs from `value`. With `null`, the field is not null.

### lt

```ts
lt<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
lt(path: DottedPath, value: Operand<QueryValue>): Condition;
```

The field is less than `value`.

### le

```ts
le<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
le(path: DottedPath, value: Operand<QueryValue>): Condition;
```

The field is less than or equal to `value`.

### gt

```ts
gt<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
gt(path: DottedPath, value: Operand<QueryValue>): Condition;
```

The field is greater than `value`.

### ge

```ts
ge<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
ge(path: DottedPath, value: Operand<QueryValue>): Condition;
```

The field is greater than or equal to `value`.

### between

```ts
between<K extends FieldNames<O>>(
  field: K,
  low: Operand<ElementOf<O[K]>>,
  high: Operand<ElementOf<O[K]>>
): Condition;
between(path: DottedPath, low: Operand<QueryValue>, high: Operand<QueryValue>): Condition;
```

The field is from `low` to `high`, both included.

### in

```ts
in<K extends FieldNames<O>>(field: K, values: readonly Operand<ElementOf<O[K]>>[]): Condition;
in(path: DottedPath, values: readonly Operand<QueryValue>[]): Condition;
```

The field equals one of `values`. Anything but an array fails with `INVALID_QUERY`.

### contains

```ts
contains<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
contains(path: DottedPath, value: Operand<QueryValue>): Condition;
```

A string field contains `value`, or a list holds the element `value`.

### startsWith

```ts
startsWith<K extends FieldNames<O>>(field: K, value: Operand<string>): Condition;
startsWith(path: DottedPath, value: Operand<string>): Condition;
```

A string field starts with `value`. On a list of strings, an element does.

### endsWith

```ts
endsWith<K extends FieldNames<O>>(field: K, value: Operand<string>): Condition;
endsWith(path: DottedPath, value: Operand<string>): Condition;
```

A string field ends with `value`. On a list of strings, an element does.

### isNull

```ts
isNull(field: FieldNames<O> | DottedPath): Condition;
```

The field is null. A list is null only when the list itself is, never when it is empty.

### isNotNull

```ts
isNotNull(field: FieldNames<O> | DottedPath): Condition;
```

The field is not null.

### and

```ts
and(...conditions: Condition[]): Condition;
```

Every one of `conditions` holds.

### or

```ts
or(...conditions: Condition[]): Condition;
```

At least one of `conditions` holds.

### not

```ts
not(condition: Condition): Condition;
```

`condition` does not hold.
