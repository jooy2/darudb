---
title: MapCollection
order: 16
group: objects
counterpart: /types/node/stored-schema
pageClass: reference-page
---

# MapCollection

`MapCollection`은 파일에 저장된 스키마의 컬렉션으로, 객체는 파일에 저장된 이름을 키로 한 맵입니다.

```dart
typedef MapCollection = CollectionSchema<Map<String, Object?>, MapQuery, Object>;
```

스키마 없이 연 데이터베이스는 파일에 저장된 스키마를 쓰고, 그 스키마의 컬렉션마다 `MapCollection`을 만듭니다. [`Database.storedCollection`](../../api/dart/database.md#storedcollection)이 이름으로 하나를 돌려주고, [`Database.storedSchema`](../../api/dart/database.md#storedschema)가 전부를 나열합니다. `darudb_generator`가 쓰는 것과 같은 [CollectionSchema](./collection-schema.md)이므로 [`txn.collection`](../../api/dart/read-transaction.md#collection)에 그대로 넘기면 됩니다. 컬렉션은 [`FieldSpec`](./field-types.md)이 설명합니다. 필드마다 이름과 종류, null 허용 여부, 기본값, 인덱스, 기본 키인지가 들어 있고, `autoKey`가 켜져 있으면 자동 증가 `id`가 맨 앞에 옵니다.

읽은 객체는 컬렉션의 모든 필드를 담은 맵입니다. null인 필드는 null이고, 내장 객체는 다시 맵이며, 링크는 [Link](./link.md)입니다. 쓰는 객체도 맵입니다.

- 맵에 없거나 null인 필드는 기본값으로 읽히고, null을 허용하는 필드면 null로 읽힙니다. 기본값이 없는 필수 필드는 꼭 넣어야 합니다.
- float 필드에는 `int`를 넣어도 되고, 링크에는 `Link`나 그 링크가 가리키는 기본 키를 넣습니다.
- 스키마에 없는 이름은 버리지 않고 `INVALID_ARGUMENT`로 거부합니다. 타입이 맞지 않는 값도 마찬가지입니다.

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

`MapCollection`의 쿼리 빌더로, `find`, `findOne`, `count`, `update`가 함수에 넘겨줍니다. 필드는 속성이 아니라 파일에 저장된 이름으로 가리키고, 어떤 메서드로 가리키느냐가 그 필드가 담는 타입을 정합니다. 그 타입에 맞는 조건과 `set`을 쓸 수 있습니다. 내장 객체의 필드나 링크가 가리키는 객체의 필드는 점으로 이어 `stringField('address.city')`처럼 씁니다. `findText` 같은 메서드에 넘기는 쿼리 언어 문자열도 필드를 같은 방식으로 가리킵니다.

| 메서드                  | 필드                                                         |
| ----------------------- | ------------------------------------------------------------ |
| `boolField(name)`       | `BoolField`                                                  |
| `intField(name)`        | `IntField`                                                   |
| `floatField(name)`      | `FloatField`                                                 |
| `stringField(name)`     | `StringField`                                                |
| `bytesField(name)`      | `BytesField`                                                 |
| `linkField(name)`       | `LinkField`                                                  |
| `listField<E>(name)`    | `ListField<E>`                                               |
| `stringListField(name)` | `StringListField`                                            |
| `objectField(name)`     | `EmbeddedField<Map<String, Object?>>`, 맵 하나로 통째로 설정 |

`objectField`는 컬렉션의 내장 객체가 아닌 이름에 `INVALID_ARGUMENT`를 던집니다. 필드 타입마다 검사하고 설정할 수 있는 것은 [필드](../../api/dart/fields.md)에 있습니다.
