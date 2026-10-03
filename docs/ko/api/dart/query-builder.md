---
title: QueryBuilder
order: 10
counterpart: [/api/rust/query, /api/node/query]
---

# QueryBuilder

`QueryBuilder`는 컬렉션 하나에 대한 쿼리의 필터와 정렬, 오프셋, 개수 제한을 담습니다.

```dart
abstract base class QueryBuilder<T>
```

`darudb_generator`가 컬렉션마다 하위 클래스를 씁니다. `User`라면 `UserQuery`이고, 필드마다 그 [필드 객체](./fields.md)를 돌려주는 getter가 있습니다. `find`, `findOne`, `count`, `update`가 하나를 새로 만들어 함수에 넘기고, 함수는 조건을 붙여 돌려줍니다. 프로그램이 직접 만들 일은 없습니다. 메서드마다 빌더를 돌려주므로 호출을 이어 쓸 수 있습니다.

```dart
final page = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q
        .where(q.age.between(18, 30))
        .where(q.name.startsWith('A') | q.email.isNull())
        .sortBy(q.age, descending: true)
        .sortBy(q.name)
        .offset(20)
        .limit(10),
  ),
);
```

쿼리는 쿼리 언어를 해석한 것과 같은 트리로 엔진에 넘어갑니다. 그래서 여기서 만든 쿼리와 같은 쿼리를 문자열로 쓴 것은 같은 객체를 찾습니다. 조건마다의 뜻과, 엔진이 무엇을 읽을지 고르는 방법은 [쿼리](../../guide/queries.md)에서 설명합니다.

## 메서드

### where

```dart
QueryBuilder<T> where(Condition condition);
```

`condition`이 참인 객체만 남깁니다. `where`를 다시 부르면 `&`처럼 두 조건이 모두 참인 객체만 남깁니다. [Condition](./fields.md#condition)은 필드 객체의 메서드로 만들고 `&`, `|`, `~`로 엮습니다.

### sortBy

```dart
QueryBuilder<T> sortBy(Field field, {bool descending = false});
```

`field`로 정렬합니다. `descending`을 주지 않으면 오름차순입니다. `sortBy`를 다시 부르면 앞의 정렬에서 같은 객체끼리 그 필드로 정렬하고, 그래도 같은 객체끼리는 기본 키 순서를 따릅니다. null은 오름차순에서 맨 앞, 내림차순에서 맨 뒤에 옵니다. 정렬이 없으면 기본 키 순서입니다. `q.address.city`처럼 내장 객체나 링크를 거친 필드로도 정렬할 수 있습니다.

### offset

```dart
QueryBuilder<T> offset(int count);
```

정렬한 뒤 쿼리가 찾은 앞쪽 `count`개를 건너뜁니다. 음수를 주면 `INVALID_ARGUMENT`를 던집니다.

### limit

```dart
QueryBuilder<T> limit(int count);
```

많아야 `count`개만 남깁니다. 음수를 주면 `INVALID_ARGUMENT`를 던집니다. 인덱스가 있는 필드 하나로만 정렬한 쿼리는 개수 제한에 이르면 읽기를 멈춥니다.
