---
title: WriteTransaction
order: 6
---

# WriteTransaction

A `WriteTransaction` holds changes to the database that commit together when the function `Database.write` runs it in returns.

```ts
interface WriteTransaction<S>
```

`Database.write` begins one, passes it to its function, commits it when the function returns and aborts it when the function throws, so nothing a throwing function did is kept. The transaction reads the commit it began at together with its own changes. A write that is refused, with `DUPLICATE_KEY` or `INVALID_ARGUMENT`, changes nothing, and the function can go on and commit the rest. The transaction and its collections can be used only while the function runs; afterwards their reads and writes throw `CLOSED`.

One write transaction runs on a file at a time, across every process. Beginning one waits for another process's writer for up to `busyTimeout` milliseconds, 5000 by default, and then fails with `BUSY`. Within the process, write transactions do not nest: a `write` from inside another's function on the same file fails at once with `INVALID_ARGUMENT`, rather than wait for itself. The commit is durable when `write` returns, unless [WriteOptions](../../types/node/write-options.md) asked for a deferred one. `S` is the database's schema. [Transactions](../../guide/transactions.md) has the longer explanation.

```ts
db.write((txn) => {
  const users = txn.collection('users');
  const key = users.insert({ name: 'Alice', email: 'alice@example.com' });

  users.update(key, { age: 31 });
});
```

## Methods

### collection

```ts
collection<N extends NameOf<S>>(
  name: N
): WriteCollection<ObjectOf<FieldsOf<S, N>>, InsertOf<FieldsOf<S, N>>>;
```

Collection `name` of the schema, as a [WriteCollection](./write-collection.md). `N` is one of the schema's collection names, and `FieldsOf<S, N>` its fields: [ObjectOf](../../types/node/object-types.md) makes the type of an object read from them, and `InsertOf` the type of one written. It throws `INVALID_ARGUMENT` for a name the schema does not have, and when the database was opened without a schema.

## AsyncWriteTransaction

```ts
interface AsyncWriteTransaction<S> {
  collection<N extends NameOf<S>>(
    name: N
  ): AsyncWriteCollection<ObjectOf<FieldsOf<S, N>>, InsertOf<FieldsOf<S, N>>>;
}
```

The write transaction of `Database.writeAsync`. Its function may be asynchronous. The transaction commits when the function resolves, once every operation it called has settled, and aborts when the function rejects. `collection` works as above and gives an [AsyncWriteCollection](./write-collection.md#asyncwritecollection), whose operations run on the thread pool in the order they were called, whether or not each was awaited.

An operation that fails rejects its own promise and changes nothing, and the transaction still commits the rest when the function resolves. A function that must not commit after a failure lets the rejection reach its own result, by awaiting the operation. Inside the function, `write`, `writeAsync`, `sync`, `close`, `compact`, `setKey` and `setPassword` on the same file, and their twins, are refused with `INVALID_ARGUMENT`.

```ts
const carol = await db.writeAsync(async (txn) => {
  const users = txn.collection('users');
  const bob = await users.findOne((q) => q.where('name', '==', 'Bob'));

  if (bob !== null) {
    await users.update(bob.id, { age: bob.age + 1 });
  }

  return users.insert({ name: 'Carol' });
});
```
