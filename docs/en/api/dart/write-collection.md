---
title: WriteCollection
order: 9
counterpart: /api/rust/collection-writer
---

# WriteCollection

A `WriteCollection` writes the objects of one collection in a write transaction, and reads them as a `ReadCollection` does.

```dart
final class WriteCollection<T, Q extends QueryBuilder<T>, K extends Object> extends ReadCollection<T, Q, K>
```

`collection` of a write transaction or of a [MigrationContext](./migration-context.md) returns one. Reads see the transaction's own changes. The methods can be called only while the transaction's function runs; afterwards they throw `CLOSED`.

The class's types keep most objects that do not fit from compiling. What is left is checked when an object is written:

- **`INVALID_ARGUMENT`** for an object the file's schema refuses, such as a link holding a key of another type than the linked collection's, or a field another handle's migration changed.
- **`DUPLICATE_KEY`** when an insert finds its key taken, or a unique index finds one of the object's values taken.
- **A refused write changes nothing**, and the transaction can go on and commit.

[Objects](../../guide/objects.md) has the rules the engine keeps.

```dart
db.write((txn) {
  final users = txn.collection(userSchema);
  final [alice, _] = users.insertMany(const [
    User(name: 'Alice', email: 'alice@example.com', age: 31),
    User(name: 'Bob', tags: ['new']),
  ]);

  users.put(users.get(alice)!.copyWith(age: 32));
  users.update(alice, (q) => [q.email.set(null)]);
  users.delete(2);
});
```

## Methods

### insert

```dart
K insert(T object);
```

Inserts `object` and returns its primary key. In a collection keyed by an auto-increment, an object whose `id` is `null` gets the next number; one with an `id` is inserted under it.

### insertMany

```dart
List<K> insertMany(Iterable<T> objects);
```

Inserts `objects` in one call into the engine and returns their keys, in order. A refused object stops the batch with its error, and the objects before it stay inserted in the transaction, which commits them unless the function throws. Encoding a batch into one buffer and crossing into the engine once costs much less than a call for each object.

### put

```dart
K put(T object);
```

Inserts `object`, or replaces the object with its key, and returns the key. It fails as `insert` does, except that a taken key is not a failure. The object replaces the stored one whole. An object of an auto-increment collection whose `id` is `null` is inserted with the next number.

### putMany

```dart
List<K> putMany(Iterable<T> objects);
```

`put` for each object, in one call into the engine, with the batch rules of `insertMany`.

### update

```dart
bool update(K key, List<Change> Function(Q q) changes);
```

Sets the fields that `changes` gives in the object whose primary key is `key`, and returns whether there was one; when there is none, nothing is written. The function receives the query builder and returns a [Change](./fields.md#change) for each field, made by the field's `set`. The rest of the object stays as it is.

- `set(null)` makes an optional field null, and gives a field with a default its default. A required field without a default cannot be set to null.
- A list or an embedded object is replaced whole: `q.address.set(const Address(city: 'Seoul'))`.
- `set` names a field of the object itself: one reached through an embedded object or a link fails with `INVALID_ARGUMENT`.

```dart
users.update(alice, (q) => [q.age.set(37), q.email.set(null), q.tags.set(['admin'])]);
```

It is refused as `put` is, with `INVALID_ARGUMENT` or `DUPLICATE_KEY`. Changing the primary key fails with `INVALID_ARGUMENT`; the key the object already has is accepted. Only the changed fields cross into the engine, which changes the record where it lies, so an update costs less than reading the object and putting it back.

### delete

```dart
bool delete(K key);
```

Deletes the object whose primary key is `key`, with its index entries, and returns whether there was one.

## AsyncWriteCollection

```dart
final class AsyncWriteCollection<T, Q extends QueryBuilder<T>, K extends Object>
    extends AsyncReadCollection<T, Q, K> {
  Future<K> insert(T object);
  Future<List<K>> insertMany(Iterable<T> objects);
  Future<K> put(T object);
  Future<List<K>> putMany(Iterable<T> objects);
  Future<bool> update(K key, List<Change> Function(Q q) changes);
  Future<bool> delete(K key);
}
```

The collection of a write transaction of the `Future` API, with the reads of [AsyncReadCollection](./read-collection.md#asyncreadcollection). Its members do what the members above do and return a `Future`; every failure completes it with the same error, and a refused call changes nothing. Calls run in the order they were made, whether or not each was awaited, and a call that fails does not stop the ones made after it. Each call is a trip to a thread of the library and back, so a batch of objects belongs in one `insertMany` or `putMany`.

```dart
await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);

  await Future.wait([
    users.insert(const User(name: 'Dave')),
    users.update(1, (q) => [q.age.set(33)]),
    users.delete(3),
  ]);
});
```
