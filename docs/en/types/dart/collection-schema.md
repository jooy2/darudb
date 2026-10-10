---
title: CollectionSchema
order: 4
group: objects
counterpart: /types/rust/collection-type
pageClass: reference-page
---

# CollectionSchema

`CollectionSchema` is what `darudb_generator` writes for a class annotated `@Collection()`: the collection's name and fields, and how its objects are written and read.

```dart
final class CollectionSchema<T, Q extends QueryBuilder<T>, K extends Object>
```

The generator writes one constant for each collection, named after the class, such as `userSchema` for `User`. `T` is the class, `Q` the query builder written for it, and `K` the type of its primary key: `int`, `String` or `Uint8List`. A [Schema](../../api/dart/schema.md) lists the constants, and `txn.collection` and `db.prepare` take one, which is how every call knows the class of the objects it reads and writes.

```dart
// Written by `dart run build_runner build` for a class `User`.
const userSchema = CollectionSchema<User, UserQuery, int>(
  name: 'users',
  autoKey: true,
  fields: [
    FieldSpec('id', IntKind()),
    FieldSpec('name', StringKind()),
    FieldSpec('email', StringKind(), optional: true, unique: true),
    FieldSpec('age', IntKind(), defaultValue: 0, index: true),
  ],
  writeField: _$writeUser,
  read: _$readUser,
  query: UserQuery.new,
);
```

A program can write a schema constant by hand in the same form, with the functions the generator would write, but the generator keeps the fields, the class and the functions in step, which hand-written code has to do itself.

## Properties

### name

```dart
final String name;
```

The collection's name in the file.

### fields

```dart
final List<FieldSpec> fields;
```

The fields, in the order the class declares them, the auto-increment `id` first when `autoKey` is set. A field's place in this list is its slot, which the write and read functions use. Fields are matched with the file's by name, so their order is not part of the schema.

### autoKey

```dart
final bool autoKey;
```

Whether the engine assigns the primary key: an `int` field `id`, the first of `fields`.

## Methods

### newQuery

```dart
Q newQuery();
```

A new query builder, which `find`, `count` and `update` make for their function.

`encode`, `decode` and `keyOf` are for the package itself.

## EmbeddedSchema

```dart
final class EmbeddedSchema<E> {
  final List<FieldSpec> fields;
}
```

What the generator writes for a class annotated `@Embedded()`, such as `addressSchema` for `Address`: its fields, and how one is written and read. An `ObjectKind` names it.

## FieldSpec

```dart
final class FieldSpec {
  const FieldSpec(
    this.name,
    this.kind, {
    this.optional = false,
    this.defaultValue,
    this.index = false,
    this.unique = false,
    this.primaryKey = false,
  });
}
```

One field, as the class declares it.

| Field | Type | Description |
| --- | --- | --- |
| `name` | `String` | The field's name in the file: the Dart field's, or the one `@Name` gives |
| `kind` | `Kind` | What values the field holds |
| `optional` | `bool` | Whether the field may be null, which it is when a record leaves it out |
| `defaultValue` | `Object?` | What a record that leaves the field out holds, or `null` for no default |
| `index` | `bool` | Whether the field has an index |
| `unique` | `bool` | Whether the index also refuses two objects with the same value |
| `primaryKey` | `bool` | Whether the field is the collection's primary key |

## Kind

```dart
sealed class Kind
```

What values a field holds, one class for each kind:

| Kind                   | Values                                                            |
| ---------------------- | ----------------------------------------------------------------- |
| `BoolKind()`           | `true` or `false`                                                 |
| `IntKind()`            | A 64-bit integer                                                  |
| `FloatKind()`          | A 64-bit floating-point number                                    |
| `StringKind()`         | UTF-8 text                                                        |
| `BytesKind()`          | Any bytes, as a `Uint8List`                                       |
| `LinkKind(collection)` | The primary key of an object of the collection named `collection` |
| `ListKind(element)`    | A list of values of a scalar kind or of links                     |
| `ObjectKind(embedded)` | An embedded object, whose fields an `EmbeddedSchema` declares     |

## FieldSink and FieldSource

The functions the generator writes, `_$writeUser` and `_$readUser` above, write and read an object a field at a time through these two classes.

- **`FieldSink`** takes each field's value by slot: `boolean`, `int64`, `float`, `string`, `bytes`, `link`, `list` and `object`, each of which takes null for a field left out.
- **`FieldSource`** walks a record field by field. `next` moves to the next field and `slot` says which it is; `int64`, `string` and the others read its value, and the `OrNull` form of each reads one that may be null. A field the record leaves out comes with its default, or null, so a record written before a field existed still reads. A record that does not fit its layout fails with `CORRUPTED`.

Neither is meant for code other than the generated functions and hand-written ones in their form.
