---
title: QueryInput
order: 8
group: queries
pageClass: reference-page
---

# QueryInput

`QueryInput` is a query built in JavaScript, as `find`, `findOne`, `count` and `Database.prepare` take it: a function that builds one, or a `Query`.

```ts
type QueryInput<O> = ((query: Query<O>) => Query<O> | void) | Query<O>;
```

`O` is the type of the collection's objects, which holds [`where`](../../api/node/query.md) and `sortBy` to the collection's fields and to values of their types.

- **A function** receives a new [Query](../../api/node/query.md) and adds to it. It may return the query or nothing; any other value fails with `INVALID_QUERY`.
- **A `Query`**, made with `new Query<O>()`, which can be built once and passed many times.
- **Nothing**: `find()` returns every object, in primary key order, and `count()` counts them.

[ReadCollection](../../api/node/read-collection.md) also takes a query as text in the query language, with parameters, or as a [Prepared](./prepared.md) query; those are overloads of their own. [Queries](../../guide/queries.md) explains what a query can say.

```ts
const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);
```

## QueryValue

```ts
type QueryValue = boolean | number | bigint | string | Uint8Array;
```

A value a condition compares with, or a parameter's value. Its JavaScript type decides the value's type:

- A `number` that is a whole number within `Number.MAX_SAFE_INTEGER` is an int, and any other number is a float. An int field compares only with an int and a float field with any number, so `2.5` against an int field fails with `INVALID_QUERY`.
- A `bigint` is an int. One beyond 64 bits fails with `INVALID_QUERY`.
- A `boolean`, a `string` and a `Uint8Array` compare with fields of those types, and a link compares with a value of the linked collection's key type.

A list or an object is never a value. A condition on a list field compares with one element, and holds when it holds for any element. A value whose type does not fit its field fails with `INVALID_QUERY` when the query runs.

## QueryParameters

```ts
type QueryParameters = readonly (QueryValue | null)[];
```

The values of a query's parameters, in order: `$0` or `param(0)` first. A query in text and a prepared query take them after the query.

- `null` is allowed only where the parameter is compared with `==` or `!=`, which then test whether the field is null. Anywhere else it fails with `INVALID_QUERY`.
- A run that leaves a parameter without a value fails with `INVALID_QUERY`, and so do parameters that are not an array and a value that is not a `QueryValue`.

```ts
users.find('age >= $0 AND name STARTSWITH $1', [18, 'A']);
users.count('email == $0', [null]); // the users without an email
```

A value that comes from outside the program belongs in a parameter, never in the text.

## Comparison

```ts
type Comparison = '==' | '!=' | '<' | '<=' | '>' | '>=';
```

The comparison operators of `Query.where`, as in `where('age', '>=', 18)`. `==` and `!=` with `null` test whether the field is null. `where` also takes `'between'` with a pair of values, `'in'` with an array, and `'contains'`, `'startsWith'` and `'endsWith'`. An operator it does not know fails with `INVALID_QUERY`. The functions `eq`, `ne`, `lt`, `le`, `gt` and `ge` of [`conditions`](../../api/node/conditions.md) make the same six comparisons.
