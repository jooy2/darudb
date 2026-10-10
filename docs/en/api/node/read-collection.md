---
title: ReadCollection
order: 7
counterpart: /api/rust/collection-reader
---

# ReadCollection

A `ReadCollection` reads the objects of one collection in a transaction: by primary key, and by query.

```ts
interface ReadCollection<O>
```

`collection` of a read transaction returns one, and a write transaction's [WriteCollection](./write-collection.md) has every member below too, reading the transaction's own changes. `O` is the type of the collection's objects, which [ObjectOf](../../types/node/object-types.md) makes from its fields. The methods can be called only while the transaction's function runs; afterwards those that read or write throw `CLOSED`.

Objects come back as plain objects that outlive the transaction. Every field of the schema is there, a field left out when the object was written holding its default or null, and a collection without a key field gives each object its `id`. A field declared with `t.bigint()` reads as a `bigint`, and bytes as a `Uint8Array`. A value beyond 2^53 in a field declared with `t.int()` fails to read with `INVALID_ARGUMENT`.

## Properties

### name

```ts
readonly name: string;
```

The collection's name.

## Methods

### get

```ts
get(key: Key): O | null;
```

The object whose primary key is `key`, or `null`. A [Key](../../types/node/key.md) is an int, as a number or a `bigint`, a string, or bytes; a `bigint` finds the int it equals. Any other value, such as `1.5`, `true` or `null`, fails with `INVALID_ARGUMENT`.

### find

```ts
find(query?: QueryInput<O>): O[];
find(text: string, parameters?: QueryParameters): O[];
find(prepared: Prepared<O>, parameters?: QueryParameters): O[];
```

The objects a query finds, in its order. A query takes one of these forms, which [QueryInput](../../types/node/query-input.md) declares:

- **Nothing**: every object, in primary key order.
- **A function** that receives a new [Query](./query.md) and adds to it. It may return the query or nothing.
- **A `Query`** made with `new Query()`.
- **Text** in the [query language](./query.md#the-query-language), where `$0`, `$1` and on take the values of `parameters` in order. The package keeps up to 256 texts of up to 4096 characters once it has parsed them, so a text that runs again is not parsed again.
- **A prepared query** that `Database.prepare` made on this collection, with the values of its parameters.

A query that does not fit fails with `INVALID_QUERY`: a field the collection does not have, a value of another type, text that does not parse, parameters that are not an array of single values, a query prepared on another collection, or a parameter without a value.

```ts
import { Query } from 'darudb';
import type { ObjectOf } from 'darudb';

type User = ObjectOf<typeof app.collections.users.fields>;

const adults = new Query<User>().where('age', '>=', 18);

db.read((txn) => {
  const users = txn.collection('users');

  users.find();
  users.find((q) => q.where('tags', 'contains', 'new').sortBy('age', 'desc').limit(10));
  users.find(adults);
  users.find('age >= $0 AND name STARTSWITH $1', [18, 'A']);
});
```

### findOne

```ts
findOne(query?: QueryInput<O>): O | null;
findOne(text: string, parameters?: QueryParameters): O | null;
findOne(prepared: Prepared<O>, parameters?: QueryParameters): O | null;
```

The first object a query finds, or `null`. It takes the query in the same forms as `find`, and the engine stops reading at the first object. A query's own offset and limit still apply, so a limit of 0 finds nothing.

### count

```ts
count(query?: QueryInput<O>): number;
count(text: string, parameters?: QueryParameters): number;
count(prepared: Prepared<O>, parameters?: QueryParameters): number;
```

How many objects a query finds, after its offset and within its limit. Without a query it counts every object, which reads only the number the collection keeps rather than the objects.

## AsyncReadCollection

```ts
interface AsyncReadCollection<O> {
  readonly name: string;
  get(key: Key): Promise<O | null>;
  find(query?: QueryInput<O>): Promise<O[]>;
  find(text: string, parameters?: QueryParameters): Promise<O[]>;
  find(prepared: Prepared<O>, parameters?: QueryParameters): Promise<O[]>;
  findOne(query?: QueryInput<O>): Promise<O | null>;
  findOne(text: string, parameters?: QueryParameters): Promise<O | null>;
  findOne(prepared: Prepared<O>, parameters?: QueryParameters): Promise<O | null>;
  count(query?: QueryInput<O>): Promise<number>;
  count(text: string, parameters?: QueryParameters): Promise<number>;
  count(prepared: Prepared<O>, parameters?: QueryParameters): Promise<number>;
}
```

The collection of an asynchronous transaction. Its members do what the members above do and return promises, and every failure, `CLOSED` included, is a rejection with the same code. Operations run on the thread pool one at a time, in the order they were called. Those called in one turn of the event loop, or while an earlier batch is on the pool, go to the engine together as one batch, so starting many and awaiting them together costs far less than awaiting each in turn.

```ts
const found = await db.readAsync((txn) => {
  const users = txn.collection('users');

  return Promise.all([1, 2, 3].map((key) => users.get(key)));
});
```
