---
title: QueryBuilder
order: 10
counterpart: [/api/rust/query, /api/node/query]
---

# QueryBuilder

`QueryBuilder` holds the parts of a query on one collection: a filter, a sort, an offset and a limit.

```dart
abstract base class QueryBuilder<T>
```

`darudb_generator` writes a subclass for each collection, such as `UserQuery` for `User`, with a getter for each field that returns its [field object](./fields.md). `find`, `findOne`, `count` and `update` make a new one and pass it to their function, which adds to it and returns it; a program does not make one itself. Each method returns the builder, so the calls chain.

```dart
final page = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q
        .where(q.age.between(18, 30))
        .where(q.name.startsWith('A') | q.email.isNull())
        .sortBy(q.age, descending: true)
        .sortBy(q.name)
        .offset(20)
        .limit(10),
  ),
);
```

The query crosses into the engine as the same tree the query language parses into, so a query built here and the same query written as text find the same objects. [Queries](../../guide/queries.md) explains what each condition means and how the engine chooses what to read.

## Methods

### where

```dart
QueryBuilder<T> where(Condition condition);
```

Keeps the objects `condition` holds for. A second `where` keeps those both hold for, as `&` does. A [Condition](./fields.md#condition) comes from a field object's methods, combined with `&`, `|` and `~`.

### sortBy

```dart
QueryBuilder<T> sortBy(Field field, {bool descending = false});
```

Sorts by `field`, ascending unless `descending`. A second `sortBy` sorts the objects that the first leaves equal, and objects that sort equal come in primary key order. Null sorts first ascending and last descending. Without a sort, objects come in primary key order. A field through an embedded object or a link can be sorted by, such as `q.address.city`.

### offset

```dart
QueryBuilder<T> offset(int count);
```

Skips the first `count` objects the query finds, after sorting. A negative count throws `INVALID_ARGUMENT`.

### limit

```dart
QueryBuilder<T> limit(int count);
```

Keeps at most `count` objects. A negative count throws `INVALID_ARGUMENT`. A query sorted by an indexed field alone stops reading at the limit.
