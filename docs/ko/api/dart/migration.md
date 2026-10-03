---
title: Migration
order: 4
counterpart: /types/node/migration
---

# Migration

`Migration`은 스키마 버전 `version`이 바로 앞 버전에서 무엇이 바뀌는지, 엔진이 알아서 하지 못하는 부분을 적습니다.

```dart
final class Migration {
  const Migration(
    this.version, {
    this.renameCollections = const {},
    this.renameFields = const {},
    this.deleteCollections = const [],
    this.replaceFields = const {},
    this.run,
  });
}
```

[`Database.open`](./database.md#open)의 `migrations` 옵션이 이것의 목록을 받습니다. 더 낮은 스키마 버전을 가진 파일을 열면 선언한 버전까지의 모든 단계를 버전 순서대로 쓰기 트랜잭션 하나 안에서 실행합니다. 어디서든 실패하면 파일은 예전 스키마와 데이터를 그대로 유지합니다.

새 컬렉션 만들기, 선택 필드나 기본값이 있는 새 필드 추가, 새 인덱스 만들기와 없어진 인덱스 지우기, 없어진 필드 정리는 엔진이 알아서 합니다. 컬렉션이나 필드의 이름 바꾸기, 컬렉션 삭제, 타입이 바뀐 필드는 마이그레이션에 적습니다. 없어진 컬렉션이나 타입이 바뀐 필드를 적지 않으면 `INVALID_ARGUMENT`로 실패하고, 앞 버전의 스키마에 없는 이름을 적어도 마찬가지입니다. `renameFields` 없이 필드 이름만 바꾸면 필드를 지우고 새로 만든 것으로 보므로, 값은 새 이름을 따라가지 않습니다. 단계 안의 이름은 모두 그 단계 전에 파일에 있던 이름입니다. 그래서 단계에서 이름을 바꾸는 컬렉션의 필드는 컬렉션의 예전 이름 아래에 적고, `@Name`을 붙인 필드는 파일 안의 이름으로 적습니다.

아래 마이그레이션은 `users`에 `name`과 정수 `age`가 있던 버전 1의 파일을 버전 2로 옮깁니다.

```dart
@Collection('people')
class Person {
  const Person({this.id, required this.fullName, this.age = ''});

  final int? id;
  final String fullName;
  final String age;
}

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
          final before = m.previous('users', key);
          final person = people.get(key as int);

          if (before != null && person != null) {
            people.put(person.copyWith(age: '${before['age']} years'));
          }
        }
      },
    ),
  ],
);
```

## 속성

### version

```dart
final int version;
```

이 단계가 이르는 스키마 버전입니다. 2부터 선언한 스키마의 버전까지의 정수여야 하고, 다른 숫자를 주거나 같은 버전의 단계가 둘이면 `INVALID_ARGUMENT`로 실패합니다.

### renameCollections

```dart
final Map<String, String> renameCollections;
```

예전 이름별 새 컬렉션 이름입니다. 이름 바꾸기는 데이터를 옮기지 않으므로 컬렉션에 객체가 아무리 많아도 비용이 없습니다.

### renameFields

```dart
final Map<String, Map<String, String>> renameFields;
```

컬렉션별, 예전 필드 이름별 새 필드 이름입니다. 컬렉션은 단계 전의 이름으로 적습니다.

### deleteCollections

```dart
final List<String> deleteCollections;
```

이 단계에서 지울 컬렉션입니다. `run`이 실행된 뒤 객체와 인덱스를 함께 지웁니다. 마이그레이션이 커밋되기 전까지는 `run`이 `previous`로 그 객체를 읽을 수 있습니다.

### replaceFields

```dart
final Map<String, List<String>> replaceFields;
```

컬렉션별로 타입이 바뀌는 필드입니다. 컬렉션은 단계 전의 이름으로 적습니다. 교체한 필드는 필드를 지우고 같은 이름으로 새로 만든 것과 같아서, `run`이 값을 주기 전까지는 기본값이나 null을 갖습니다.

### run

```dart
final FutureOr<void> Function(MigrationContext context)? run;
```

마이그레이션의 쓰기 트랜잭션 안에서 데이터를 옮깁니다. 단계의 이름 바꾸기와 엔진이 알아서 하는 변경이 끝난 뒤 실행되며 [MigrationContext](./migration-context.md)를 받습니다. `Database.open`에서는 동기 함수여야 하고, `Database.openAsync`에서는 비동기여도 되며 단계는 함수의 `Future`가 완료되면 마무리됩니다. 함수가 예외를 던지면 `open`도 같은 예외를 던집니다.
