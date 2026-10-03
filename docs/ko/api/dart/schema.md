---
title: Schema
order: 3
---

# Schema

`Schema`는 `darudb_generator`가 쓴 스키마 상수로 데이터베이스의 컬렉션을 버전과 함께 선언하며, `Database.open`이 이것을 받습니다.

```dart
final class Schema {
  const Schema(this.version, this.collections);

  final int version;
  final List<CollectionSchema<Object?, QueryBuilder<Object?>, Object>> collections;
}
```

`version`은 1 이상의 정수이고, 스키마를 바꿀 때마다 올립니다. `collections`에는 컬렉션마다 [CollectionSchema](../../types/dart/collection-schema.md)를 넣습니다. `@Collection()`을 붙인 클래스 `User`라면 `userSchema`입니다. 링크가 가리키는 컬렉션도 모두 목록에 있어야 합니다. 스키마는 상수이므로 여러 번 열 때 함께 써도 됩니다.

```dart
const app = Schema(1, [teamSchema, userSchema]);

final db = Database.open('app.darudb', schema: app);
```

엔진이 저장할 수 없는 스키마는 열 때 `INVALID_ARGUMENT`로 실패합니다. 1보다 작은 버전, 이름이 같은 컬렉션 둘, 스키마에 없는 컬렉션을 가리키는 링크가 그 예입니다. 기본 키가 둘인 컬렉션이나 내장 객체 안의 인덱스 같은 나머지는 프로그램을 실행하기 전에 생성기가 거부합니다.

처음 열 때 스키마를 파일에 저장하고, 그 뒤로는 열 때마다 선언한 스키마를 저장된 것과 비교합니다.

- **버전도 내용도 같으면** 할 일이 없습니다. 필드는 이름으로 맞추므로, 컬렉션을 넣은 순서나 필드를 선언한 순서만 바꾼 것은 변경이 아닙니다.
- **버전은 같은데 내용이 다르면** `SCHEMA_MISMATCH`입니다. 버전을 올리지 않고 스키마를 바꿨기 때문입니다.
- **파일의 버전이 더 높으면** `SCHEMA_TOO_NEW`입니다. 더 새로운 애플리케이션이 쓴 파일이기 때문입니다.
- **파일의 버전이 더 낮으면** 마이그레이션합니다. [Migration](./migration.md)과 가이드의 [마이그레이션](../../guide/migrations.md)에서 설명합니다.

## 속성

### version

```dart
final int version;
```

스키마 버전입니다.

### collections

```dart
final List<CollectionSchema<Object?, QueryBuilder<Object?>, Object>> collections;
```

받은 순서 그대로의 컬렉션입니다.
