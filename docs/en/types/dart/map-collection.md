---
title: MapCollection
order: 16
group: objects
counterpart: /types/node/stored-schema
pageClass: reference-page
---

# MapCollection

`MapCollection` is a collection of the schema a file holds, whose objects are maps by the names the file stores.

```dart
typedef MapCollection = CollectionSchema<Map<String, Object?>, MapQuery, Object>;
```

A database opened without a schema works with the one the file holds, and makes a `MapCollection` for each of its collections from it: [`Database.storedCollection`](../../api/dart/database.md#storedcollection) gives one by name, and [`Database.storedSchema`](../../api/dart/database.md#storedschema) lists them all. It is a [CollectionSchema](./collection-schema.md) like the one `darudb_generator` writes, so [`txn.collection`](../../api/dart/read-transaction.md#collection) takes it, and its [`FieldSpec`](./field-types.md)s describe the collection: each field's name and kind, whether it is optional, its default and its index, and which is the primary key, with the auto-increment `id` first when `autoKey` is set.

An object read is a map with every field of the collection, null where a field is null, an embedded object a map of its own, and a link a [Link](./link.md). An object written is a map too:

- A field the map leaves out, or holds null in, reads as its default, or as null where it is optional; a required field without a default has to be there.
- A float field takes an `int`, and a link takes a `Link` or the primary key it holds.
- A name the schema does not have is refused with `INVALID_ARGUMENT`, rather than dropped, and so is a value of the wrong type.

```dart
final db = Database.open('app.darudb');
final users = db.storedCollection('users');

db.write((txn) {
  final collection = txn.collection(users);
  final id = collection.insert({'name': 'Ada', 'age': 36, 'tags': <String>[]});

  collection.update(id, (q) => [q.intField('age').set(37)]);
});

for (final field in users.fields) {
  print('${field.name}: ${field.kind.runtimeType}, optional: ${field.optional}');
}
```

## MapQuery

```dart
final class MapQuery extends QueryBuilder<Map<String, Object?>>
```

The query builder of a `MapCollection`, which `find`, `findOne`, `count` and `update` hand their functions. A field is named by the name the file stores rather than by a property, and the method says what it holds, which gives the conditions and the `set` that type has. A dot separates the fields of an embedded object or of the object a link points to, as `stringField('address.city')`. Text in the query language, through `findText` and the rest, names fields the same way.

| Method                  | Field                                                     |
| ----------------------- | --------------------------------------------------------- |
| `boolField(name)`       | `BoolField`                                               |
| `intField(name)`        | `IntField`                                                |
| `floatField(name)`      | `FloatField`                                              |
| `stringField(name)`     | `StringField`                                             |
| `dateField(name)`       | `DateField`                                               |
| `bytesField(name)`      | `BytesField`                                              |
| `linkField(name)`       | `LinkField`                                               |
| `listField<E>(name)`    | `ListField<E>`                                            |
| `stringListField(name)` | `StringListField`                                         |
| `objectField(name)`     | `EmbeddedField<Map<String, Object?>>`, set whole as a map |

`objectField` throws `INVALID_ARGUMENT` for a name that is not an embedded object of the collection. [Fields](../../api/dart/fields.md) has what each field type can test and set.
