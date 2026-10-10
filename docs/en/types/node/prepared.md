---
title: Prepared
order: 9
group: queries
pageClass: reference-page
---

# Prepared

`Prepared` is a query parsed once, on one collection, that each run gives values for its parameters.

```ts
interface Prepared<O> {
  readonly collection: string;
  readonly [objects]?: O;
}
```

[`Database.prepare`](../../api/node/database.md) makes one, from text in the query language with `$0`, `$1` and on, or from a query built with [`param`](../../api/node/param.md) in place of the values that change. Nothing else makes one. `find`, `findOne` and `count` take it with the values of its parameters as [QueryParameters](./query-input.md#queryparameters).

`O` is the type of the objects the query finds, so the type checker refuses a prepared query on a collection of another type. `[objects]` is the property that carries `O`: only the type checker knows it, and it never exists at run time.

A prepared query holds no database and no transaction, so it runs in any transaction, synchronous or asynchronous, read or write. Preparing saves parsing or encoding the query on every run, which matters most for text. The engine still plans each run for the values it is given.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const inAges = db.prepare('users', 'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) => {
  const users = txn.collection('users');

  users.findOne(byEmail, ['alice@example.com']);
  users.find(inAges, [18, 30]);
});
```

The package also keeps text passed straight to `find`, `findOne` or `count` prepared: up to 256 texts in all, each of up to 4096 characters and kept with the collection it ran on. `prepare` still pays for a query built in JavaScript, which is otherwise encoded on every run, and for a program that runs more texts than the package keeps.

- `prepare` fails with `INVALID_ARGUMENT` for a collection the schema does not have, with `INVALID_QUERY` for text that does not parse, and with `CLOSED` after the database is closed.
- A run fails with `INVALID_QUERY` on another collection, when it leaves a parameter without a value, and when a field or a value of the query does not fit the schema.

## Properties

### collection

```ts
readonly collection: string;
```

The collection the query was prepared on, and the only one it runs on.
