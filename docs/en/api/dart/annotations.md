---
title: Annotations
order: 2
counterpart: /api/rust/derive
---

# Annotations

The annotations make a Dart class the objects of a collection, or an embedded object, and `darudb_generator` writes the code that stores it.

```yaml
dependencies:
  darudb:
    path: ../darudb/packages/dart/darudb

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator:
    path: ../darudb/packages/dart/darudb_generator
```

A library with annotated classes declares a part named after it, and `dart run build_runner build` writes that part. The generator reads only the annotations, and nothing in the program calls it at run time.

```dart
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'models.g.dart';

@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  @Name('postcode')
  final String? zip;
}

@Collection('users')
class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.tags = const [],
    this.avatar,
    this.address,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  @Index()
  final List<String> tags;
  final Uint8List? avatar;
  final Address? address;
}

@Collection('posts')
class Post {
  const Post({required this.slug, required this.author, this.readers = const []});

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User> author;
  final List<Link<User>> readers;
}

final db = Database.open('app.darudb', schema: const Schema(1, [userSchema, postSchema]));
```

## The class

An annotated class is a value, which the generated code builds with its constructor and reads through its fields:

- **Every field is `final`**, and the class has no type parameters.
- **The unnamed constructor takes every field**, by name or by position, as a parameter of the same name. The fields are the instance fields the class declares itself: a getter, a static field and an inherited field are not stored.
- **A field's type is its type in the file**: `bool`, `int`, `double`, `String`, `Uint8List`, a `List` of those or of links, a [Link](../../types/dart/link.md) to a collection, or a class annotated `@Embedded()`. A list holds no null. [Field types](../../types/dart/field-types.md) has the whole list.
- **Nullable means optional.** A nullable field may be null, and is null when a record leaves it out. A field that is not nullable is required.
- **A parameter's default is the field's default**, which a record written before the field existed reads as. It is a constant the file can store: a `bool`, an `int`, a `double`, a `String`, or a list of them. A field with a default is required, so it is not nullable.

The generator stops with a message naming the class and the field when one of these does not hold.

## Each annotation

### Collection

```dart
const Collection([String? name]);
```

Makes the class the objects of a collection named `name`, or named as the class is when `name` is left out. Without a field annotated `@PrimaryKey()`, the collection is keyed by an auto-increment, and the class needs a field `final int? id`, with no other annotation. It is `null` in an object that has not been inserted, and the engine gives the next number to an object inserted with it `null`.

### Embedded

```dart
const Embedded();
```

Makes the class an embedded object, which a field of another object holds. An embedded object has no key, and none of its fields can be indexed.

### PrimaryKey

```dart
const PrimaryKey();
```

Makes the field the collection's primary key. It is an `int`, a `String` or a `Uint8List`, required and without a default, and a collection has one at most.

### Index

```dart
const Index();
```

Keeps an index on the field, so that a query with a condition on it reads only the objects it finds, and a query sorted by it alone reads them in that order. An index on a list has an entry for each element.

### Unique

```dart
const Unique();
```

Keeps an index on the field that also refuses two objects with the same value, with `DUPLICATE_KEY`. Any number of objects may hold null.

### Name

```dart
const Name(String name);
```

Names the field `name` in the file, rather than as the Dart field is named. A query names the Dart field; the query language names the field in the file.

## What the generator writes

For a class `User` annotated `@Collection()`:

| Generated | What it is |
| --- | --- |
| `userSchema` | The collection's [CollectionSchema](../../types/dart/collection-schema.md), the constant a [Schema](./schema.md) lists and `txn.collection` takes |
| `UserQuery` | The collection's [QueryBuilder](./query-builder.md), with a [field object](./fields.md) for each field |
| `UserLink` | The field object of a link to a `User`, with `User`'s fields, which a query reads through the link |
| `UserCopyWith` | An extension with `copyWith`, which copies an object with the fields it is given changed |

For a class `Address` annotated `@Embedded()`, it writes `addressSchema`, an [EmbeddedSchema](../../types/dart/collection-schema.md#embeddedschema), `AddressFields`, the field object of an embedded `Address`, and `AddressCopyWith`.

`copyWith` keeps every field it is not given, so it cannot make a field null: construct the object for that, or use `update` with `set(null)`.
