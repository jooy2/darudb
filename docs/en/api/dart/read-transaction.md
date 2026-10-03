---
title: ReadTransaction
order: 6
---

# ReadTransaction

A `ReadTransaction` reads one commit of the database, for as long as the function `Database.read` runs it in.

```dart
base class ReadTransaction
```

`Database.read` begins one, passes it to its function, and ends it when the function returns or throws. Everything it reads comes from the commit that was published when it began: commits made meanwhile, by this process or another, are not seen. Beginning one waits for no writer, and no writer waits for it. The transaction and the collections it gives can be used only while the function runs; afterwards every call on them throws `CLOSED`.

Keep read transactions short where others write. While one is open, the pages that later commits stop using cannot be reused, so writers take new pages and the file grows. [Transactions](../../guide/transactions.md) has the longer explanation.

```dart
final names = db.read(
  (txn) => txn.collection(userSchema).find((q) => q.sortBy(q.name)).map((user) => user.name).toList(),
);
```

## Methods

### collection

```dart
ReadCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> schema,
);
```

The collection of `schema`, as a [ReadCollection](./read-collection.md) whose objects are of the class `T`, whose queries `Q` builds and whose primary key is a `K`. The type arguments come from the schema constant, such as `userSchema`, so they are never written out. It throws `INVALID_ARGUMENT` for a collection the database was not opened with. If another process or handle has migrated the file since this handle opened it, reading the collection fails with `SCHEMA_MISMATCH`, and the database has to be opened again with the new schema.

## AsyncReadTransaction

```dart
base class AsyncReadTransaction {
  AsyncReadCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
  );
}
```

The read transaction of `Database.readAsync`. Its function may be asynchronous, and the transaction sees one commit until the function completes. `collection` works as above and gives an [AsyncReadCollection](./read-collection.md#asyncreadcollection), whose calls return a `Future` and run on threads of the native library, one at a time, in the order they were made. The transaction ends once the function has completed and every call it made has finished; a call made after that fails with `CLOSED`.

```dart
final (alice, adults) = await db.readAsync((txn) async {
  final users = txn.collection(userSchema);

  return (await users.get(1), await users.count((q) => q.where(q.age.atLeast(18))));
});
```
