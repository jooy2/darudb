---
title: Prepared
order: 6
group: queries
pageClass: reference-page
---

# Prepared

`Prepared` is a query in the query language, parsed once, on one collection, that each run gives values for its parameters.

```dart
final class Prepared<T> implements Finalizable
```

[`Database.prepare`](../../api/dart/database.md#prepare) makes one, from text with `$0`, `$1` and on in place of the values that change. Nothing else makes one. `findPrepared`, `findOnePrepared` and `countPrepared` take it with the values of its parameters. `T` is the class of the objects the query finds, so a prepared query on a collection of another class does not compile.

A prepared query holds no transaction, so it runs in any transaction of the database, synchronous or asynchronous, read or write. Preparing saves parsing the text on every run. The engine still plans each run for the values it is given. The native query it holds is freed when the garbage collector reclaims it.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');
final inAges = db.prepare(userSchema, r'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) {
  final users = txn.collection(userSchema);

  users.findOnePrepared(byEmail, ['alice@example.com']);
  users.findPrepared(inAges, [18, 30]);
});
```

The package also keeps text passed straight to `findText`, `findOneText` or `countText` prepared: up to 256 texts for each database, kept with the collection they ran on. `prepare` still pays for a program that runs more texts than the package keeps.

- `prepare` fails with `INVALID_QUERY` for text that does not parse, and with `CLOSED` after the database is closed.
- A run fails with `INVALID_ARGUMENT` on another collection, and with `INVALID_QUERY` when it leaves a parameter without a value, or when a field or a value of the query does not fit the schema.
