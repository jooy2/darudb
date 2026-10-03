---
title: ReadCollection
order: 8
counterpart: /api/rust/collection-reader
---

# ReadCollection

`ReadCollection`은 트랜잭션 안에서 컬렉션 하나의 객체를 기본 키나 쿼리로 읽습니다.

```dart
base class ReadCollection<T, Q extends QueryBuilder<T>, K extends Object>
```

읽기 트랜잭션의 `collection`이 돌려주며, 쓰기 트랜잭션의 [WriteCollection](./write-collection.md)에도 아래 멤버가 모두 있어서 트랜잭션의 변경까지 읽습니다. `T`는 컬렉션 객체의 클래스, `Q`는 `darudb_generator`가 그 클래스에 맞춰 쓴 쿼리 빌더, `K`는 기본 키의 타입으로 `int`, `String`, `Uint8List` 중 하나입니다. 메서드는 트랜잭션의 함수가 도는 동안에만 부를 수 있고, 그 뒤에는 `CLOSED`를 던집니다.

객체는 클래스의 생성자로 만든 인스턴스로 돌아오며, 트랜잭션이 끝나도 남습니다. 필드가 생기기 전에 써서 레코드에 없는 필드에는 기본값이나 null이 들어가고, 자동 증가 키를 쓰는 컬렉션의 객체에는 `id`가 들어갑니다.

## 속성

### name

```dart
String get name;
```

컬렉션 이름입니다.

## 메서드

### get

```dart
T? get(K key);
```

기본 키가 `key`인 객체를 돌려줍니다. 없으면 `null`입니다.

### find

```dart
List<T> find([QueryBuilder<T> Function(Q q)? query]);
```

쿼리가 찾은 객체를 쿼리의 순서대로 돌려줍니다. 쿼리가 없으면 모든 객체를 기본 키 순서로 돌려줍니다. 함수는 새 [쿼리 빌더](./query-builder.md)를 받아 조건을 붙여 돌려주며, `q.age`처럼 빌더로 각 필드에 닿습니다.

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  users.find();
  users.find((q) => q.where(q.tags.contains('new')).sortBy(q.age, descending: true).limit(10));
  users.find((q) => q.where(q.age.atLeast(18) & ~q.email.isNull()));
});
```

파일과 맞지 않는 쿼리는 `INVALID_QUERY`로 실패합니다. 다른 핸들의 마이그레이션이 지운 필드를 가리키는 쿼리가 그 예입니다. 나머지는 타입이 막아서 컴파일되지 않습니다.

### findOne

```dart
T? findOne([QueryBuilder<T> Function(Q q)? query]);
```

쿼리가 찾은 첫 객체를 돌려줍니다. 없으면 `null`입니다. 엔진은 첫 객체에서 읽기를 멈춥니다. 쿼리에 준 오프셋과 개수 제한은 그대로 적용되므로, 개수 제한이 0이면 아무것도 찾지 않습니다.

### count

```dart
int count([QueryBuilder<T> Function(Q q)? query]);
```

쿼리가 찾은 객체의 개수를 오프셋과 개수 제한을 적용해 돌려줍니다. 쿼리가 없으면 모든 객체를 세는데, 이때는 객체를 읽지 않고 컬렉션이 기록해 둔 개수만 읽습니다.

### findText

```dart
List<T> findText(String text, [List<Object?> parameters = const []]);
```

[쿼리 언어](../../guide/queries.md#문자열로-쿼리-쓰기)로 쓴 `text`가 찾은 객체를 돌려줍니다. `$0`, `$1` 같은 매개변수에는 `parameters`의 값이 차례로 들어가며, 값은 `bool`, `int`, `double`, `String`, `Uint8List`, [Link](../../types/dart/link.md)입니다. 패키지는 한 번 해석한 문자열을 256개까지 기억해 두므로, 같은 문자열을 다시 실행할 때는 해석을 건너뜁니다. `r'...'`처럼 원시 문자열로 써야 Dart가 `$0`을 문자열 보간으로 읽지 않습니다.

해석할 수 없는 문자열, 컬렉션에 없는 필드, 타입이 다른 값, 값이 없는 매개변수는 `INVALID_QUERY`로 실패합니다.

```dart
users.findText(r'age >= $0 AND name STARTSWITH $1 SORT BY age DESC', [18, 'A']);
```

### findOneText

```dart
T? findOneText(String text, [List<Object?> parameters = const []]);
```

`text`가 찾은 첫 객체를 돌려줍니다. 없으면 `null`입니다.

### countText

```dart
int countText(String text, [List<Object?> parameters = const []]);
```

`text`가 찾은 객체의 개수를 돌려줍니다.

### findPrepared

```dart
List<T> findPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
```

[`Database.prepare`](./database.md#prepare)로 준비한 쿼리가 찾은 객체를 돌려줍니다. `parameters`에는 매개변수 값을 넣습니다. 다른 컬렉션에서 준비한 쿼리면 `INVALID_ARGUMENT`로 실패합니다.

### findOnePrepared

```dart
T? findOnePrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
```

준비한 쿼리가 찾은 첫 객체를 돌려줍니다. 없으면 `null`입니다.

### countPrepared

```dart
int countPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
```

준비한 쿼리가 찾은 객체의 개수를 돌려줍니다.

## AsyncReadCollection

```dart
base class AsyncReadCollection<T, Q extends QueryBuilder<T>, K extends Object> {
  String get name;
  Future<T?> get(K key);
  Future<List<T>> find([QueryBuilder<T> Function(Q q)? query]);
  Future<T?> findOne([QueryBuilder<T> Function(Q q)? query]);
  Future<int> count([QueryBuilder<T> Function(Q q)? query]);
  Future<List<T>> findText(String text, [List<Object?> parameters = const []]);
  Future<T?> findOneText(String text, [List<Object?> parameters = const []]);
  Future<int> countText(String text, [List<Object?> parameters = const []]);
  Future<List<T>> findPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
  Future<T?> findOnePrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
  Future<int> countPrepared(Prepared<T> prepared, [List<Object?> parameters = const []]);
}
```

`Future` API 트랜잭션의 컬렉션입니다. 멤버는 위의 멤버와 같은 일을 하고 `Future`를 돌려주며, `CLOSED`를 비롯한 모든 실패는 같은 오류로 `Future`를 완료합니다. 호출은 네이티브 라이브러리의 스레드에서 부른 순서대로 하나씩 실행되고, 호출마다 한 번씩 스레드를 오갑니다.

```dart
final found = await db.readAsync((txn) {
  final users = txn.collection(userSchema);

  return Future.wait([1, 2, 3].map(users.get));
});
```
