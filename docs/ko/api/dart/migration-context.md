---
title: MigrationContext
order: 5
counterpart: [/api/rust/migrating, /api/node/migrating]
---

# MigrationContext

`MigrationContext`는 마이그레이션 함수가 받는 것으로, 새 스키마의 컬렉션과 마이그레이션 전 스키마로 읽은 객체를 줍니다.

```dart
final class MigrationContext
```

`Database.open`은 파일의 스키마 버전이 더 낮으면 쓰기 트랜잭션 하나 안에서 파일을 마이그레이션합니다. 먼저 단계마다 이름을 바꾸고 엔진이 알아서 하는 변경을 한 뒤, 두 버전 사이의 [Migration](./migration.md)마다 `run` 함수를 버전 순서대로 `MigrationContext`와 함께 부릅니다. 함수가 예외를 던지면 마이그레이션은 하나도 남지 않고, 파일은 예전 스키마와 데이터를 그대로 유지하며, `open`도 같은 예외를 던집니다. 엔진이 알아서 하는 변경과 함수가 필요한 변경은 [마이그레이션](../../guide/migrations.md)에서 설명합니다.

`Database.openAsync`에서 함수 자체는 비동기여도 되지만, 이 객체의 메서드는 언제나 동기입니다. 마이그레이션은 커밋할 때까지 파일의 쓰기를 쥐고 있으므로, 메서드가 다른 쓰기를 기다릴 일은 없습니다. 반대로 함수 안에서 다른 핸들로 같은 파일에 쓰면 그 쓰기가 마이그레이션을 기다리게 되니 그러지 마세요. 이 객체와 그 컬렉션은 마이그레이션이 진행되는 동안에만 쓸 수 있고, 그 뒤에는 모든 호출이 `CLOSED`를 던집니다.

```dart
final db = Database.open(
  'app.darudb',
  schema: const Schema(2, [personSchema]),
  migrations: [
    Migration(
      2,
      renameCollections: const {'users': 'people'},
      renameFields: const {
        'users': {'name': 'fullName'},
      },
      replaceFields: const {
        'users': ['age'],
      },
      run: (m) {
        final people = m.collection(personSchema);

        for (final key in m.previousKeys('users')) {
          final before = m.previous('users', key)!;

          people.update(key as int, (q) => [q.age.set('${before['age']} years')]);
        }
      },
    ),
  ],
);
```

`Person`은 `age`가 문자열이 된 [Migration](./migration.md) 예제의 클래스입니다.

## 속성

### previousVersion

```dart
int get previousVersion;
```

마이그레이션 전에 파일에 있던 스키마 버전입니다. 모든 단계에서 같습니다.

## 메서드

### collection

```dart
WriteCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> schema,
);
```

새 스키마에 있는 `schema`의 컬렉션을, 마이그레이션의 트랜잭션을 읽고 쓰는 [WriteCollection](./write-collection.md)으로 돌려줍니다. 쿼리는 쿼리 빌더로 만듭니다. 마이그레이션 안에서 `findText`와 `countText`는 `INVALID_ARGUMENT`로 실패합니다. 새 스키마에 없는 컬렉션이면 `INVALID_ARGUMENT`로 실패합니다.

### previous

```dart
Map<String, Object?>? previous(String name, Object key);
```

컬렉션 `name`에서 기본 키가 `key`인 객체를 마이그레이션 전 스키마로 읽어 돌려줍니다. 없으면 `null`입니다. 컬렉션과 객체의 필드는 마이그레이션 전에 파일에서 쓰던 이름이고, 마이그레이션이 지우거나 교체한 필드도 값을 그대로 갖습니다. 레코드에 없는 필드에는 기본값이나 null이 들어갑니다. 내장 객체는 따로 맵이 되고, 링크는 담긴 키가 되며, 목록은 `List`가 됩니다. 마이그레이션이 지울 컬렉션도 커밋되기 전까지는 이렇게 읽을 수 있습니다. 예전 스키마에 없던 컬렉션이면 `INVALID_ARGUMENT`로 실패합니다.

지금 상태의 객체를 읽으며, 객체를 쓰면 새 스키마의 필드만 남습니다. 그러니 객체를 쓰기 전에 이렇게 읽어 두세요. 예전 스키마의 클래스는 보통 프로그램에 남아 있지 않으므로 객체를 맵으로 돌려줍니다.

### previousKeys

```dart
List<Object> previousKeys(String name);
```

컬렉션 `name`에 있는 모든 객체의 기본 키를 키 순서대로 돌려줍니다. 컬렉션은 마이그레이션 전의 이름으로 적습니다. 키는 컬렉션의 키 타입에 따라 `int`, `String`, `Uint8List`입니다.
