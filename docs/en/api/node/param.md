---
title: param
order: 11
---

# param

`param` puts a parameter in place of a value in a query that `Database.prepare` prepares, so that each run of the query gives the value.

```ts
const param: (index: number) => Param;
```

`param(0)` is the first parameter, the same as `$0` in the query language, and `param(1)` the second. A parameter can stand for any value a condition compares with: the value of `where`, either end of `between`, an element of `in`. It cannot stand for a field, an operator, a sort, a limit or an offset. `index` has to be a whole number from 0 up, or `param` throws `INVALID_QUERY`.

A query with parameters has to be prepared with [Database.prepare](./database.md#prepare); run as it is, it fails with `INVALID_QUERY`. Each `find`, `findOne` or `count` of the [Prepared](../../types/node/prepared.md) query gives the values in an array, in order, as [QueryParameters](../../types/node/query-input.md). A parameter left without a value, or a value of another type than the field's, fails with `INVALID_QUERY`. `null` for a parameter of `==` or `!=` tests whether the field is null, as `null` written in the query would.

Preparing saves building, encoding and parsing the query on every run; the engine still plans each run for the values it is given. A prepared query runs only on the collection it was prepared on, and in any transaction, synchronous or asynchronous, read or write.

```ts
import { param } from 'darudb';

const inAges = db.prepare('users', (q) =>
  q.where('age', 'between', [param(0), param(1)]).sortBy('age')
);
const byEmail = db.prepare('users', 'email == $0');

db.read((txn) => {
  const users = txn.collection('users');

  users.find(inAges, [18, 30]);
  users.findOne(byEmail, ['alice@example.com']);
  users.count(byEmail, [null]); // the users without an email
});
```

`param` returns a frozen `Param`:

```ts
interface Param
```

## Properties

### index

```ts
readonly index: number;
```

The parameter's number, counted from 0.
