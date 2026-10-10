---
title: ReadTransaction
order: 5
---

# ReadTransaction

A `ReadTransaction` reads one commit of the database, for as long as the function `Database.read` runs it in.

```ts
interface ReadTransaction<S>
```

`Database.read` begins one, passes it to its function, and ends it when the function returns or throws. Everything it reads comes from the commit that was published when it began: commits made meanwhile, by this process or another, are not seen. Beginning one waits for no writer, and no writer waits for it. The transaction and the collections it gives can be used only while the function runs; afterwards their reads and writes throw `CLOSED`.

Keep read transactions short where others write. While one is open, the pages that later commits stop using cannot be reused, so writers take new pages and the file grows. `S` is the database's schema, which gives each collection the type of its objects. [Transactions](../../guide/transactions.md) has the longer explanation.

```ts
const names = db.read((txn) =>
  txn
    .collection('users')
    .find((q) => q.sortBy('name'))
    .map((user) => user.name)
);
```

## Methods

### collection

```ts
collection<N extends NameOf<S>>(name: N): ReadCollection<ObjectOf<FieldsOf<S, N>>>;
```

Collection `name` of the schema, as a [ReadCollection](./read-collection.md). `N` is one of the schema's collection names, and `FieldsOf<S, N>` its fields, of which [ObjectOf](../../types/node/object-types.md) makes the type of its objects. It throws `INVALID_ARGUMENT` for a name the schema does not have, and when the database was opened without a schema. If another process or handle has migrated the file since this handle opened it, reading the collection fails with `SCHEMA_MISMATCH`, and the database has to be opened again with the new schema.

## AsyncReadTransaction

```ts
interface AsyncReadTransaction<S> {
  collection<N extends NameOf<S>>(name: N): AsyncReadCollection<ObjectOf<FieldsOf<S, N>>>;
}
```

The read transaction of `Database.readAsync`. Its function may be asynchronous, and the transaction sees one commit until the function settles. `collection` works as above and gives an [AsyncReadCollection](./read-collection.md#asyncreadcollection), whose operations return promises and run on the thread pool, one at a time, in the order they were called. The transaction ends once the function has settled and every operation it called has settled too; an operation called after that rejects with `CLOSED`.

```ts
const [alice, adults] = await db.readAsync((txn) => {
  const users = txn.collection('users');

  return Promise.all([users.get(1), users.count((q) => q.where('age', '>=', 18))]);
});
```
