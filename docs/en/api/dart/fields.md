---
title: Fields
order: 11
counterpart: [/api/rust/filter, /api/node/conditions]
---

# Fields

A field object names one field in a query, and its methods make the conditions a filter is made of and the changes `update` makes.

```dart
abstract base class Field {
  final List<String> path;
}
```

The query builder `darudb_generator` writes has a getter for each field of the class, which returns the field object for the field's type. A program reaches them only through the builder its function receives, such as `q.age`.

| Field's Dart type | Field object |
| --- | --- |
| `bool` | `BoolField` |
| `int` | `IntField` |
| `double` | `FloatField` |
| `String` | `StringField` |
| `Uint8List` | `BytesField` |
| `List<String>` | `StringListField` |
| Any other `List` | `ListField<E>`, where `E` is the element type, and `Object` for a list of links |
| `Link<User>` | `UserLink`, a `LinkField` with `User`'s fields |
| An `@Embedded()` class `Address` | `AddressFields`, an `EmbeddedField<Address>` with `Address`'s fields |

A field of an embedded object or of a linked object is reached through the field that holds it: `q.address.city`, or `q.team.city` to test the linked object. A link to an object that is not there reads as null. [Queries](../../guide/queries.md) explains what each condition means for lists and for null.

```dart
final found = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q.where((q.age.atLeast(18) & q.age.lessThan(30)) | q.email.isNull()),
  ),
);
```

## Condition

```dart
final class Condition {
  Condition operator &(Condition other);
  Condition operator |(Condition other);
  Condition operator ~();
}
```

A condition of a query's filter, which a field object's method makes. `a & b` holds when both hold, `a | b` when either holds, and `~a` when `a` does not. Dart gives `~` the tightest binding and `&` a tighter one than `|`, so `a | b & ~c` is `a | (b & (~c))`; parentheses make any other grouping. A condition is a value and can go into any number of queries.

A filter nests at most 24 levels deep, or the query fails with `INVALID_QUERY`. A run of `&`, or a run of `|`, is one level, so only `~` and alternating groups count.

## Change

```dart
final class Change
```

One field's new value, for [`update`](./write-collection.md#update), which a field object's `set` makes. `set` names a field of the object itself: one reached through an embedded object or a link throws `INVALID_ARGUMENT`.

## Methods of every field

### isNull

```dart
Condition isNull();
```

Holds when the field is null. A list that is empty is not null.

### isNotNull

```dart
Condition isNotNull();
```

Holds when the field is not null.

## Methods of value fields

`BoolField`, `IntField`, `FloatField`, `StringField` and `BytesField` take values of their field's type, `V`.

### equals

```dart
Condition equals(V value);
```

Holds when the field equals `value`.

### notEquals

```dart
Condition notEquals(V value);
```

Holds when the field does not equal `value`, or is null.

### isIn

```dart
Condition isIn(List<V> values);
```

Holds when the field equals one of `values`.

### set

```dart
Change set(V? value);
```

The change that sets the field to `value`. Null makes an optional field null, and gives a field with a default its default.

## Methods of ordered fields

`IntField`, `FloatField`, `StringField` and `BytesField` have an order: numbers by value, strings and bytes by their bytes.

### lessThan

```dart
Condition lessThan(V value);
```

Holds when the field is below `value`.

### atMost

```dart
Condition atMost(V value);
```

Holds when the field is `value` or below.

### greaterThan

```dart
Condition greaterThan(V value);
```

Holds when the field is above `value`.

### atLeast

```dart
Condition atLeast(V value);
```

Holds when the field is `value` or above.

### between

```dart
Condition between(V low, V high);
```

Holds when the field is from `low` to `high`, both included.

## Methods of string fields

### contains

```dart
Condition contains(String text);
```

Holds when the field contains `text`.

### startsWith

```dart
Condition startsWith(String text);
```

Holds when the field starts with `text`.

### endsWith

```dart
Condition endsWith(String text);
```

Holds when the field ends with `text`.

## Methods of list fields

A condition on a list holds when it holds for any element. `ListField<E>` and `StringListField` have these.

### contains

```dart
Condition contains(E value);
```

Holds when the list has an element equal to `value`. In a list of links, `value` is the linked object's key.

### containsAny

```dart
Condition containsAny(List<E> values);
```

Holds when an element equals one of `values`.

### anyStartsWith

```dart
Condition anyStartsWith(String text);
```

Of `StringListField` only: holds when an element starts with `text`.

### anyEndsWith

```dart
Condition anyEndsWith(String text);
```

Of `StringListField` only: holds when an element ends with `text`.

### set

```dart
Change set(List<E>? values);
```

The change that replaces the list with `values`, or makes it null.

## Methods of embedded object fields

An `EmbeddedField<E>` holds an embedded object of the class `E`, and its subclass has that class's fields, which a condition reads through it.

### set

```dart
Change set(E? value);
```

The change that replaces the embedded object whole with `value`, or makes it null. Null gives a required field with a default its default. The object is written with its fields where the file keeps them, so a class whose fields are declared in another order than the file's still writes each to its own.

## Methods of link fields

A `LinkField` compares the key the link holds, of the linked collection's key type, and its subclass has the linked collection's fields besides, which a condition reads through the link.

### equals

```dart
Condition equals(Object key);
```

Holds when the link holds `key`.

### notEquals

```dart
Condition notEquals(Object key);
```

Holds when the link does not hold `key`, or is null.

### isIn

```dart
Condition isIn(List<Object> keys);
```

Holds when the link holds one of `keys`.

### set

```dart
Change set(Link<Object?>? link);
```

The change that sets the link, or makes it null.
