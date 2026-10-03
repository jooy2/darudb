---
title: Query
order: 9
---

# Query

A `Query` says which objects a collection's `find`, `findOne` and `count` look for, in what order, and how many.

```ts
interface Query<O = Record<string, unknown>>
```

A query has a filter, a sort, an offset and a limit, each optional. `find`, `findOne` and `count` hand a new one to the function they are given; `new Query<O>()` makes one to keep, pass around or prepare. Each method adds to the query and returns the same query, so a query kept and run again runs with everything added to it so far. Without a sort, objects come in primary key order, and objects that sort equal come in primary key order too.

`O` is the type of the objects the query finds. In the function form it is the collection's, so `where` and `sortBy` accept only its fields and values of their types. `new Query()` without a type takes any field and any value, and the engine checks them when the query runs, as it checks every dotted path. A query that does not fit the schema fails with `INVALID_QUERY` when it runs.

The signatures use four types that the package declares for its own use: `FieldNames<O>` is the names of `O`'s fields, `DottedPath` is any string with a `.` in it, `ElementOf<T>` is a field's type without null, or the element type of a list, and `Operand<T>` is `T` or a [Param](./param.md) of a prepared query.

```ts
import { Query } from 'darudb';
import type { ObjectOf } from 'darudb';

type User = ObjectOf<typeof app.collections.users.fields>;

const query = new Query<User>().where('age', '>=', 18).sortBy('name').limit(20);
const adults = db.read((txn) => txn.collection('users').find(query));
```

A condition on the primary key or on an indexed field, joined to the rest of the filter with `and`, lets the engine read only the objects that meet it, and a sort by one indexed field alone reads in that order and stops at the limit. The result is the same either way. [Queries](../../guide/queries.md) has the longer explanation.

## Methods

### where

```ts
where<K extends FieldNames<O>>(
  field: K,
  op: '==' | '!=',
  value: Operand<ElementOf<O[K]> | null>
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: Comparison,
  value: Operand<ElementOf<O[K]>>
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: 'between',
  value: readonly [Operand<ElementOf<O[K]>>, Operand<ElementOf<O[K]>>]
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: 'in',
  value: readonly Operand<ElementOf<O[K]>>[]
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: 'contains' | 'startsWith' | 'endsWith',
  value: Operand<ElementOf<O[K]>>
): Query<O>;
where(
  path: DottedPath,
  op: Comparison | 'contains' | 'startsWith' | 'endsWith',
  value: Operand<QueryValue | null>
): Query<O>;
where(
  path: DottedPath,
  op: 'between',
  value: readonly [Operand<QueryValue>, Operand<QueryValue>]
): Query<O>;
where(path: DottedPath, op: 'in', value: readonly Operand<QueryValue>[]): Query<O>;
where(condition: Condition | ((conditions: Conditions<O>) => Condition)): Query<O>;
```

Keeps the objects that meet a condition, and every condition given to `where` before: each call is joined to the earlier ones with `and`. The condition takes one of three forms.

- **A field, an operator and a value.** The operators are the [Comparison](../../types/node/query-input.md) operators `==`, `!=`, `<`, `<=`, `>` and `>=`, then `between` with a pair `[low, high]`, `in` with an array, and `contains`, `startsWith` and `endsWith`. `== null` tests that the field is null, and `!= null` that it is not.
- **A dotted path** in place of the field, through an embedded object or a link, such as `address.city` or `team.city`. TypeScript does not check it; the engine does.
- **A condition** from [conditions](./conditions.md), or a function that receives the conditions typed for the collection and returns one. This is the form for `or`, `not`, `isNull` and nested groups.

An operator that is not one of these, or a `between` without a pair, fails with `INVALID_QUERY`.

```ts
db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null));
  users.find((q) => q.where('age', 'between', [18, 30]).where('tags', 'contains', 'new'));
  users.find((q) => q.where('address.city', '==', 'Seoul'));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
});
```

### sortBy

```ts
sortBy(field: FieldNames<O> | DottedPath, direction?: 'asc' | 'desc'): Query<O>;
```

Sorts by `field`, ascending unless `direction` is `'desc'`, after any sort given before. Null sorts first ascending and last descending, and strings sort by their UTF-8 bytes. A direction other than `'asc'` or `'desc'` fails with `INVALID_QUERY`, and so does sorting by a list, by an embedded object, or through a to-many link.

### limit

```ts
limit(count: number): Query<O>;
```

Returns at most `count` objects. A later call replaces an earlier one. `count` has to be a whole number from 0 up, or the query fails with `INVALID_QUERY` when it runs.

### offset

```ts
offset(count: number): Query<O>;
```

Skips the first `count` objects of the sorted result. A later call replaces an earlier one, and `count` follows the rule of `limit`.

## The query language

`find`, `findOne`, `count` and `Database.prepare` also take a query as text, which the engine parses into the same query a builder makes. The collection is not part of the text: the call that runs it names it.

```ts
db.read((txn) =>
  txn
    .collection('users')
    .find('age >= $0 AND (name STARTSWITH "A" OR email IS NULL) SORT BY age DESC LIMIT 10', [18])
);
```

- **Order.** A filter comes first, then `SORT BY` with fields separated by commas, each ascending unless `DESC` follows it, then `LIMIT` and `OFFSET`. Each part may be left out.
- **Conditions.** `field == value`, with `!=`, `<`, `<=`, `>` and `>=` too; `field BETWEEN a AND b`; `field IN [a, b]`; `field CONTAINS value`, `STARTSWITH` and `ENDSWITH`; `field IS NULL` and `field IS NOT NULL`. They join with `AND`, `OR`, `NOT` and parentheses, and `AND` binds tighter than `OR`. A field may be a dotted path.
- **Values.** An int has no point, and a float has one or an exponent; either may start with `-`. Strings are in double quotes, with the escapes `\"`, `\\`, `\n`, `\t` and `\u{...}`. The other values are `true`, `false` and `null`.
- **Parameters.** `$0`, `$1` and on take the values passed with the text, in order. A value that comes from outside the program belongs in a parameter, never in the text.
- **Names.** Keywords are case-insensitive. A field named like a keyword, such as `limit`, goes in backticks where a path starts.
- **Errors.** Text that does not parse fails with `INVALID_QUERY`, and the message names the character, counted from 1, where it went wrong. Parentheses and `NOT` nest at most 48 levels deep.
