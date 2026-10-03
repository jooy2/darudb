---
title: ReadCollection
order: 8
counterpart: /api/rust/collection-reader
---

# ReadCollection

A `ReadCollection` reads the objects of one collection in a transaction: by primary key, and by query.

```dart
base class ReadCollection<T, Q extends QueryBuilder<T>, K extends Object>
```

`collection` of a read transaction returns one, and a write transaction's [WriteCollection](./write-collection.md) has every member below too, reading the transaction's own changes. `T` is the class of the collection's objects, `Q` the query builder `darudb_generator` wrote for it, and `K` the type of its primary key: `int`, `String` or `Uint8List`. The methods can be called only while the transaction's function runs; afterwards they throw `CLOSED`.

Objects come back as instances of the class, built with its constructor, that outlive the transaction. A field a record leaves out, written before the field existed, holds its default or null, and a collection keyed by an auto-increment gives each object its `id`.

## Properties

### name

```dart
String get name;
```

The collection's name.

## Methods

### get

```dart
T? get(K key);
```

The object whose primary key is `key`, or `null`.

### find

```dart
List<T> find([QueryBuilder<T> Function(Q q)? query]);
```

The objects a query finds, in its order, or every object in primary key order without one. The function receives a new [query builder](./query-builder.md), adds to it and returns it, and reaches each field through it, such as `q.age`.

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  users.find();
  users.find((q) => q.where(q.tags.contains('new')).sortBy(q.age, descending: true).limit(10));
  users.find((q) => q.where(q.age.atLeast(18) & ~q.email.isNull()));
});
```

A query that does not fit the file fails with `INVALID_QUERY`, such as one that names a field another handle's migration removed. The types keep the rest from compiling.

### findOne

```dart
T? findOne([QueryBuilder<T> Function(Q q)? query]);
```

The first object a query finds, or `null`. The engine stops reading at the first object. A query's own offset and limit still apply, so a limit of 0 finds nothing.

### count

```dart
int count([QueryBuilder<T> Function(Q q)? query]);
```

How many objects a query finds, after its offset and within its limit. Without a query it counts every object, which reads only the number the collection keeps rather than the objects.

### findText

```dart
List<T> findText(String text, [List<Object?> parameters = const []]);
```

The objects that `text`, a query in the [query language](../../guide/queries.md#write-a-query-as-text), finds, where `$0`, `$1` and on take the values of `parameters` in order: `bool`, `int`, `double`, `String`, `Uint8List` or [Link](../../types/dart/link.md) values. The package keeps up to 256 texts once it has parsed them, so a text that runs again is not parsed again. A raw string, `r'...'`, keeps Dart from reading `$0` as interpolation.

It fails with `INVALID_QUERY` for text that does not parse, a field the collection does not have, a value of another type, or a parameter without a value.

```dart
users.findText(r'age >= $0 AND name STARTSWITH $1 SORT BY age DESC', [18, 'A']);
```

### findOneText

```dart
T? findOneText(String text, [List<Object?> parameters = const []]);
```

The first object `text` finds, or `null`.

### countText

```dart
int countText(String text, [List<Object?> parameters = const []]);
```

How many objects `text` finds.

### findPrepared

```dart
List<T> findPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
```

The objects a query that [`Database.prepare`](./database.md#prepare) made finds, with `parameters` for its parameters. A query prepared on another collection fails with `INVALID_ARGUMENT`.

### findOnePrepared

```dart
T? findOnePrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
```

The first object a prepared query finds, or `null`.

### countPrepared

```dart
int countPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
```

How many objects a prepared query finds.

## AsyncReadCollection

```dart
base class AsyncReadCollection<T, Q extends QueryBuilder<T>, K extends Object> {
  String get name;
  Future<T?> get(K key);
  Future<List<T>> find([QueryBuilder<T> Function(Q q)? query]);
  Future<T?> findOne([QueryBuilder<T> Function(Q q)? query]);
  Future<int> count([QueryBuilder<T> Function(Q q)? query]);
  Future<List<T>> findText(String text, [List<Object?> parameters = const []]);
  Future<T?> findOneText(String text, [List<Object?> parameters = const []]);
  Future<int> countText(String text, [List<Object?> parameters = const []]);
  Future<List<T>> findPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
  Future<T?> findOnePrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
  Future<int> countPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
}
```

The collection of a transaction of the `Future` API. Its members do what the members above do and return a `Future`, and every failure, `CLOSED` included, completes the `Future` with the same error. Calls run on threads of the native library one at a time, in the order they were made, each a trip of its own.

```dart
final found = await db.readAsync((txn) {
  final users = txn.collection(userSchema);

  return Future.wait([1, 2, 3].map(users.get));
});
```
