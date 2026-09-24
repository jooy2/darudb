---
title: Node.js
order: 4
---

# Node.js

The Node.js package reaches the same engine and the same file as Rust, through a synchronous API: a schema declared in JavaScript, transactions scoped to a function, and queries built with a typed builder or written as text.

## Declare a schema

`t` has the field types, `collection` groups fields, and `schema` gives the collections a version. The TypeScript types of every object follow from the declaration.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  teams: collection({
    name: t.string().primaryKey(),
    city: t.string().optional()
  }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index(),
    tags: t.list(t.string()).optional().index(),
    team: t.link('teams').optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
  })
});

const db = Database.open('app.darudb', { schema: app });
```

- The types are `t.bool()`, `t.int()`, `t.bigint()`, `t.float()`, `t.string()`, `t.bytes()`, `t.link(collection)`, `t.list(type)` and `t.object(fields)`.
- `optional()` lets a field be null, `default(value)` fills it in when it is left out, `index()` and `unique()` index it, and `primaryKey()` makes it the key. A collection without a key field gets an `id` that the engine numbers from 1.
- A `t.int()` field holds a number. A value beyond 2^53, which a number does not hold exactly, is refused when written and fails when read; declare such a field with `t.bigint()`, which always reads as a `bigint`. Bytes are a `Uint8Array`.

[Collections and objects](./objects.md) has the rules the engine keeps, which are the same in every language.

## Read and write

`write` runs a function in a write transaction and commits when the function returns; if it throws, nothing it did is kept. `read` runs a function in a read transaction, which sees one commit for as long as the function runs. Both return what the function returns, and neither lets a transaction outlive its function.

```ts
db.write((txn) => {
  txn.collection('teams').insert({ name: 'north', city: 'Seoul' });

  const users = txn.collection('users');

  users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31, team: 'north' },
    { name: 'Bob', tags: ['new'] }
  ]);
  users.put({ id: 2, name: 'Robert', age: 18 });
  users.delete(3);
});

const alice = db.read((txn) => txn.collection('users').get(1));
```

- `insert` and `insertMany` return the keys. `put` and `putMany` insert or replace. `delete` says whether there was an object.
- A batch crosses into the engine as one buffer in one call, which is much cheaper than one call per object.
- `insert` fails with `DUPLICATE_KEY` when the key or a unique value is taken. A value of the wrong type, or a property the schema does not have, fails with `INVALID_ARGUMENT`. A refused write changes nothing, and the rest of the function can go on.
- Transactions are synchronous. A function that returns a promise is refused, and its transaction is aborted. Write transactions do not nest: `db.write` inside another's function fails at once, where it would otherwise wait for itself. A write waits for another process's writer for up to five seconds, or the `busyTimeout` option in milliseconds.
- `db.write(fn, { durability: 'deferred' })` returns without waiting for the disk. Readers see the changes at once, and they become durable at the next sync commit, `db.sync()`, `close`, or within a second.

## Query

`find`, `findOne` and `count` take a function that builds a query. `findOne` stops at the first object.

```ts
const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);

db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null));
  users.find((q) => q.where('tags', 'contains', 'new').where('age', 'between', [18, 30]));
  users.find((q) => q.where('team.city', '==', 'Seoul'));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
  users.count((q) => q.where('name', 'startsWith', 'A'));
});
```

In TypeScript, `where` accepts only the collection's fields, and a value of the field's type; a path through an embedded object or a link, such as `team.city`, is checked by the engine when the query runs. The same query can be written as text, with parameters:

```ts
users.find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [18, 'A']);
```

A value that comes from outside the program belongs in a parameter, never in the text.

## Migrate

Raise the schema's version when it changes. The engine adds new collections, fields with a default and indexes by itself; anything else is a migration, and a migration can run a JavaScript function in the migration's write transaction.

```ts
const app2 = schema(2, {
  teams: collection({ name: t.string().primaryKey(), city: t.string().optional() }),
  people: collection({
    fullName: t.string(),
    email: t.string().optional().unique(),
    age: t.string().default('')
  })
});

const db = Database.open('app.darudb', {
  schema: app2,
  migrations: [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      run(m) {
        const people = m.collection('people');

        for (const key of m.previousKeys('users')) {
          const before = m.previous('users', key);
          const person = people.get(key);

          if (before !== null && person !== null) {
            people.put({ ...person, age: `${before.age} years` });
          }
        }
      }
    }
  ]
});
```

`previous` reads an object as the schema before the migration did, with its old names and the values of replaced fields; read an object that way before writing it. If the function throws, the file keeps its old schema and data, and `open` throws the same error.

## Errors

Every error the package throws is an `Error` whose `code` is one of the engine's codes, listed in [Getting started](./getting-started.md#errors). A transaction or collection used after its function has returned throws `CLOSED`.
