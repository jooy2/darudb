---
title: CollectionSchema
order: 4
group: objects
counterpart: /types/rust/collection-type
pageClass: reference-page
---

# CollectionSchema

`CollectionSchema`는 `darudb_generator`가 `@Collection()`을 붙인 클래스에 써 주는 것으로, 컬렉션의 이름과 필드, 객체를 쓰고 읽는 방법을 담습니다.

```dart
final class CollectionSchema<T, Q extends QueryBuilder<T>, K extends Object>
```

생성기는 컬렉션마다 클래스 이름을 딴 상수를 하나 씁니다. `User`라면 `userSchema`입니다. `T`는 클래스, `Q`는 그 클래스에 맞춰 쓴 쿼리 빌더, `K`는 기본 키의 타입으로 `int`, `String`, `Uint8List` 중 하나입니다. [Schema](../../api/dart/schema.md)가 이 상수를 나열하고, `txn.collection`과 `db.prepare`가 이 상수를 받습니다. 그래서 모든 호출이 읽고 쓰는 객체의 클래스를 압니다.

```dart
// `dart run build_runner build`가 클래스 `User`에 써 준 코드입니다.
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

생성기가 쓸 함수와 함께 같은 모양으로 스키마 상수를 직접 써도 됩니다. 다만 생성기는 필드와 클래스, 함수를 서로 맞춰 주지만, 직접 쓴 코드는 그것을 스스로 지켜야 합니다.

## 속성

### name

```dart
final String name;
```

파일 안의 컬렉션 이름입니다.

### fields

```dart
final List<FieldSpec> fields;
```

클래스가 선언한 순서대로의 필드입니다. `autoKey`가 참이면 자동 증가 키 `id`가 맨 앞에 옵니다. 목록에서의 자리가 필드의 슬롯이고, 쓰기 함수와 읽기 함수가 이 슬롯을 씁니다. 파일의 필드와는 이름으로 맞추므로 순서는 스키마의 일부가 아닙니다.

### autoKey

```dart
final bool autoKey;
```

엔진이 기본 키를 정하는지 나타냅니다. 그렇다면 키는 `fields`의 첫 필드인 `int` 필드 `id`입니다.

## 메서드

### newQuery

```dart
Q newQuery();
```

새 쿼리 빌더를 돌려줍니다. `find`, `count`, `update`가 함수에 넘길 빌더를 이것으로 만듭니다.

`encode`, `decode`, `keyOf`는 패키지가 안에서 쓰는 메서드입니다.

## EmbeddedSchema

```dart
final class EmbeddedSchema<E> {
  final List<FieldSpec> fields;
}
```

생성기가 `@Embedded()`를 붙인 클래스에 써 주는 것으로, 필드와 객체를 쓰고 읽는 방법을 담습니다. `Address`라면 `addressSchema`입니다. `ObjectKind`가 이것을 가리킵니다.

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

클래스가 선언한 필드 하나입니다.

| 필드           | 타입      | 설명                                                                |
| -------------- | --------- | ------------------------------------------------------------------- |
| `name`         | `String`  | 파일 안의 필드 이름. Dart 필드 이름이거나 `@Name`이 정한 이름입니다 |
| `kind`         | `Kind`    | 필드가 담는 값의 종류                                               |
| `optional`     | `bool`    | 필드가 null일 수 있는지. 레코드에 없으면 null이 됩니다              |
| `defaultValue` | `Object?` | 필드가 없는 레코드가 갖는 값. 기본값이 없으면 `null`입니다          |
| `index`        | `bool`    | 필드에 인덱스가 있는지                                              |
| `unique`       | `bool`    | 인덱스가 값이 같은 객체 둘을 거부하는지                             |
| `primaryKey`   | `bool`    | 필드가 컬렉션의 기본 키인지                                         |

## Kind

```dart
sealed class Kind
```

필드가 담는 값의 종류이며, 종류마다 클래스가 하나씩 있습니다.

| 종류                   | 값                                                 |
| ---------------------- | -------------------------------------------------- |
| `BoolKind()`           | `true`나 `false`                                   |
| `IntKind()`            | 64비트 정수                                        |
| `FloatKind()`          | 64비트 부동소수점 수                               |
| `StringKind()`         | UTF-8 문자열                                       |
| `BytesKind()`          | 임의의 바이트. `Uint8List`로 다룹니다              |
| `LinkKind(collection)` | 이름이 `collection`인 컬렉션에 있는 객체의 기본 키 |
| `ListKind(element)`    | 스칼라 값이나 링크의 목록                          |
| `ObjectKind(embedded)` | 내장 객체. 필드는 `EmbeddedSchema`가 선언합니다    |

## FieldSink와 FieldSource

생성기가 쓰는 함수, 위의 `_$writeUser`와 `_$readUser`는 이 두 클래스로 객체를 필드 하나씩 쓰고 읽습니다.

- **`FieldSink`는** 슬롯마다 필드 값을 받습니다. `boolean`, `int64`, `float`, `string`, `bytes`, `link`, `list`, `object`가 있고, 모두 레코드에서 뺄 필드를 null로 받습니다.
- **`FieldSource`는** 레코드를 필드 하나씩 훑습니다. `next`로 다음 필드로 가고 `slot`으로 어느 필드인지 봅니다. 값은 `int64`, `string` 같은 메서드로 읽고, null일 수 있는 값은 각 메서드의 `OrNull` 형태로 읽습니다. 레코드에 없는 필드는 기본값이나 null로 오므로, 필드가 생기기 전에 쓴 레코드도 읽힙니다. 레이아웃과 맞지 않는 레코드는 `CORRUPTED`로 실패합니다.

둘 다 생성된 함수와, 그와 같은 모양으로 직접 쓴 함수에서만 쓰라고 있는 클래스입니다.
