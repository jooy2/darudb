---
title: WriteTransaction
order: 7
---

# WriteTransaction

A `WriteTransaction` holds changes to the database that commit together when the function `Database.write` runs it in returns.

```dart
final class WriteTransaction extends ReadTransaction
```

`Database.write` begins one, passes it to its function, commits it when the function returns and aborts it when the function throws, so nothing a throwing function did is kept. The transaction reads the commit it began at together with its own changes. A write that is refused, with `DUPLICATE_KEY` or `INVALID_ARGUMENT`, changes nothing, and the function can go on and commit the rest. The transaction and its collections can be used only while the function runs; afterwards every call on them throws `CLOSED`.

One write transaction runs on a file at a time, across every process. Beginning one waits for another process's writer for up to `busyTimeout`, five seconds by default, and then fails with `BUSY`. Within the isolate, write transactions do not nest: a `write` from inside another's function on the same file fails at once with `INVALID_ARGUMENT`, rather than wait for itself. The commit is durable when `write` returns, unless [Durability](../../types/dart/durability.md) asked for a deferred one. [Transactions](../../guide/transactions.md) has the longer explanation.

```dart
db.write((txn) {
  final users = txn.collection(userSchema);
  final key = users.insert(const User(name: 'Alice', email: 'alice@example.com'));

  users.update(key, (q) => [q.age.set(31)]);
});
```

## Methods

### collection

```dart
WriteCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> schema,
);
```

The collection of `schema`, as a [WriteCollection](./write-collection.md), which reads the transaction's own changes and makes more. It throws `INVALID_ARGUMENT` for a collection the database was not opened with.

## AsyncWriteTransaction

```dart
final class AsyncWriteTransaction extends AsyncReadTransaction {
  AsyncWriteCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
  );
}
```

The write transaction of `Database.writeAsync`. Its function may be asynchronous. The transaction commits when the function completes, once every call it made has finished, and aborts when the function fails. `collection` works as above and gives an [AsyncWriteCollection](./write-collection.md#asyncwritecollection), whose calls run on threads of the native library in the order they were made, whether or not each was awaited.

A call that fails completes its own `Future` with the error and changes nothing, and the transaction still commits the rest when the function completes. A function that must not commit after a failure lets the error reach its own result, by awaiting the call. Inside the function, `write`, `sync`, `close`, `compact`, `setKey` and `setPassword` on the same file, and their `Async` twins, are refused with `INVALID_ARGUMENT`.

```dart
final carol = await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);
  final bob = await users.findOne((q) => q.where(q.name.equals('Bob')));

  if (bob != null) {
    await users.update(bob.id!, (q) => [q.age.set(bob.age + 1)]);
  }

  return users.insert(const User(name: 'Carol'));
});
```
