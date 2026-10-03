---
title: Field types
order: 5
counterpart: /types/rust/field-type
---

# Field types

A field's Dart type decides what the field holds in the file, whether it may be null, and which field object a query reaches it through.

| Dart type | Kind in the file | Field object |
| --- | --- | --- |
| `bool` | `BoolKind` | `BoolField` |
| `int` | `IntKind`, 64 bits | `IntField` |
| `double` | `FloatKind`, 64 bits | `FloatField` |
| `String` | `StringKind` | `StringField` |
| `Uint8List` | `BytesKind` | `BytesField` |
| `List<E>` of those | `ListKind` | `ListField<E>`, or `StringListField` for `List<String>` |
| `Link<User>` | `LinkKind('users')` | `UserLink` |
| `List<Link<User>>` | `ListKind(LinkKind('users'))` | `ListField<Object>` |
| An `@Embedded()` class `Address` | `ObjectKind(addressSchema)` | `AddressFields` |

```dart
@Collection('users')
class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.tags = const [],
    this.team,
    this.address,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  final List<String> tags;
  final Link<Team>? team;
  final Address? address;
}
```

## Required, optional and default

- **Required**: a field whose type is not nullable, such as `String name`. Every object has it.
- **Optional**: a nullable field, such as `String? email`. It may be null, and a record written before the field existed reads it as null.
- **Default**: a field that is not nullable and whose constructor parameter has a default, such as `this.age = 0`. It is required, and a record written before the field existed reads it as the default. The default is a constant the file stores: a `bool`, an `int`, a `double`, a `String`, or a list of them.

A field cannot be nullable and have a default, and a list holds no null. The generator refuses both.

## Values

- **Integers** are 64-bit, as a Dart `int` is on every platform the package runs on.
- **Strings** are stored as UTF-8 and compared by their bytes.
- **Bytes** come back as a `Uint8List` of their own, which a program may change without changing the database.
- **Lists** come back as growable lists of their own.
- **Embedded objects** come back as instances of their class, built with its constructor.
- **Links** come back as [Link](./link.md) values, whose `key` is of the linked collection's key type.
