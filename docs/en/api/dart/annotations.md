---
title: Annotations
order: 2
counterpart: [/api/rust/derive, /api/node/collection]
---

# Annotations

The annotations make a Dart class the objects of a collection, or an embedded object, and `darudb_generator` writes the code that stores it.

```yaml
dependencies:
  darudb: ^1.6.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.6.0
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
- **The unnamed constructor takes every field**, by name or by position, as a parameter of the same name. The fields are the instance fields the class declares itself: a getter, a static field and an inherited field are not stored. A constructor that takes an inherited field as `super.field` is refused, since that field would be left out without a word: declare the field in the class itself.
- **A field's type is its type in the file**: `bool`, `int`, `double`, `String`, `Uint8List`, `DateTime`, a `List` of those, of links or of embedded classes, a [Link](../../types/dart/link.md) to a collection, or a class annotated `@Embedded()`. A list holds no null. [Field types](../../types/dart/field-types.md) has the whole list.
- **Nullable means optional.** A nullable field may be null, and is null when a record leaves it out. A field that is not nullable is required.
- **A parameter's default is the field's default**, which a record written before the field existed reads as. It is a constant the file can store: a `bool`, an `int`, a `double`, a `String`, or a list of them. A field with a default is required, so it is not nullable.
- **A field's name is public, and not a member of the generated query classes.** A private field is refused, since the generated code names every field. So is a field named `path`, `isNull`, `isNotNull`, `equals`, `notEquals`, `isIn`, `set`, `where`, `sortBy`, `offset`, `limit` or `linkedFrom`, which the classes the generated ones extend already have: give the Dart field another name, and keep the stored one with `@Name('limit')`.

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

Makes the class an embedded object, which a field of another object holds. An embedded object has no key, and its fields take no `@Index()` or `@Unique()`; the collection that holds it indexes them by their paths, with [`@IndexOn`](#indexon).

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

### TextIndex

```dart
const TextIndex();
```

Keeps a text index on the field, a `String` or a `List<String>`, nullable or not: an entry for each different word of the text, which a search for words, [`matches`](./fields.md#matches) or `MATCHES` in the query language, reads rather than every object. The field may be annotated [Index](#index) too, for an index on its values.

```dart
@Collection('articles')
class Article {
  const Article({this.id, required this.title, this.body, this.tags = const []});

  final int? id;
  @Index()
  @TextIndex()
  final String title;
  @TextIndex()
  final String? body;
  @TextIndex()
  final List<String> tags;
}
```

A word is a run of letters, digits and marks, compared ignoring case and by its first 64 characters; Chinese characters, hiragana and katakana are indexed two at a time. Every write of an object writes an entry for each different word, so a long text costs more to write than any other field. The generator refuses it on a field of another type, on the auto-increment `id`, and in an embedded class, whose fields get one by their paths with [IndexOn](#indexon). A file with a text index is in object format 3, which earlier releases do not open; `upgradeFormat: false` in [open](./database.md#open) refuses a schema that adds one.

### Cascade

```dart
const Cascade();
```

Deleting the object the `Link` field names deletes the object that holds it, in the same transaction, and so on down the chain. Only a `Link`, nullable or not, of a collection cascades: the generator refuses it on any other type, on a `List` of links, and in an embedded class. Annotate the field [Index](#index) too: without an index, each delete of an object it can name reads the whole collection. A file with a cascading link is in object format 2. The guide has [the rules](../../guide/objects.md#delete-what-links-to-an-object).

### IndexOn

```dart
const IndexOn(List<String> fields, {bool unique = false, bool text = false});
```

On a class annotated `@Collection()`, keeps an index on `fields`, in order, whose entries sort by the first field, then by the second, and so on. Each is a field's Dart name, or Dart names joined by `.` through embedded objects, such as `'home.city'`, and an index has one to eight of them. A class may carry several.

```dart
@Collection('posts')
@IndexOn(['author', 'written'])
@IndexOn(['place.city'])
class Post {
  const Post({this.id, required this.author, required this.written, required this.place});

  final int? id;
  final String author;
  final DateTime written;
  final Place place;
}
```

A query reads the index when it tests the first fields for equality, with `equals`, `isNull` or `isIn`, and the next one, if any, for a range or with `startsWith`. The index gives its objects in order when the query sorts by every field after those it tests for equality, in the index's order: a query for one author's posts sorted by `written` descending, with a limit of 20, reads twenty entries and stops. With `unique: true`, two objects cannot hold the same values in all the fields, though any number may hold null in one of them, and a write that would repeat them fails with `DUPLICATE_KEY`. The generator stops on a field the class does not have, a path through a link, and an index with no field or with an empty path. An index on more than eight fields, or on several fields one of which is a path through a list, is refused by the engine when the database opens, with `INVALID_ARGUMENT`. A file with such an index is in object format 2, which the `upgradeFormat` option of [open](./database.md#open) governs.

With `text: true`, the index is a text index, as [TextIndex](#textindex) keeps on a field, on the one field named, a `String` or a `List<String>` inside an embedded object: `@IndexOn(['margin.note'], text: true)`. The generator stops on one that is unique, on several fields, or on a field that holds no text. A file with one is in object format 3.

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

For a class `Address` annotated `@Embedded()`, it writes `addressSchema`, an [EmbeddedSchema](../../types/dart/collection-schema.md#embeddedschema), `AddressFields`, the field object of an embedded `Address`, `AddressListFields`, the field object of a list of them, and `AddressCopyWith`.

`copyWith` keeps every field it is not given, so it cannot make a field null: construct the object for that, or use `update` with `set(null)`.
