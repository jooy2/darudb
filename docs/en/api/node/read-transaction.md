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

Collection `name` of the schema, as a [ReadCollection](./read-collection.md). `N` is one of the schema's collection names, and `FieldsOf<S, N>` its fields, of which [ObjectOf](../../types/node/object-types.md) makes the type of its objects. A database opened without a schema reaches the collections of the file's own by name, with plain objects. It throws `INVALID_ARGUMENT` for a name the schema does not have, and when the database has no schema at all, as when the file holds none; a stored schema that does not read fails with `CORRUPTED` or `UNSUPPORTED_FORMAT_VERSION`. If another process or handle has migrated the file since this handle opened it, reading the collection fails with `SCHEMA_MISMATCH`, and the database has to be opened again with the new schema.

## Properties

### commitId

```ts
readonly commitId: number;
```

The id of the commit the transaction sees. It only grows from one commit to the next, so a larger one than an earlier transaction saw says that something was committed in between, by this process or another. A program that keeps what it read, a list on the screen for one, compares the two to tell whether to read again. A [WriteTransaction](./write-transaction.md) has no such property.

```ts
let seen = 0;

const changed = db.read((txn) => {
  const changed = txn.commitId !== seen;

  seen = txn.commitId;

  return changed;
});
```

## AsyncReadTransaction

```ts
interface AsyncReadTransaction<S> {
  collection<N extends NameOf<S>>(name: N): AsyncReadCollection<ObjectOf<FieldsOf<S, N>>>;
  readonly commitId: number;
}
```

The read transaction of `Database.readAsync`. Its function may be asynchronous, and the transaction sees one commit until the function settles. `collection` works as above and gives an [AsyncReadCollection](./read-collection.md#asyncreadcollection), whose operations return promises and run on the thread pool, one at a time, in the order they were called. The transaction ends once the function has settled and every operation it called has settled too; an operation called after that rejects with `CLOSED`. `commitId` is as above, read without a trip to the thread pool.

```ts
const [alice, adults] = await db.readAsync((txn) => {
  const users = txn.collection('users');

  return Promise.all([users.get(1), users.count((q) => q.where('age', '>=', 18))]);
});
```
