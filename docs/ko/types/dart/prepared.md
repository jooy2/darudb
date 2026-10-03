---
title: Prepared
order: 6
---

# Prepared

`Prepared`는 컬렉션 하나에 대해 한 번 해석해 두고, 실행할 때마다 매개변수의 값을 주는 쿼리 언어 쿼리입니다.

```dart
final class Prepared<T> implements Finalizable
```

[`Database.prepare`](../../api/dart/database.md#prepare)가 바뀌는 값 자리에 `$0`, `$1` 같은 매개변수를 쓴 문자열로 만듭니다. 다른 방법으로는 만들 수 없습니다. `findPrepared`, `findOnePrepared`, `countPrepared`가 이 쿼리와 함께 매개변수의 값을 받습니다. `T`는 쿼리가 찾는 객체의 클래스여서, 클래스가 다른 컬렉션에서 준비한 쿼리를 넘기면 컴파일되지 않습니다.

준비한 쿼리는 트랜잭션을 붙잡고 있지 않습니다. 그래서 그 데이터베이스라면 동기든 비동기든, 읽기든 쓰기든 어느 트랜잭션에서나 실행할 수 있습니다. 미리 준비해 두면 실행할 때마다 문자열을 해석하는 비용이 빠집니다. 실행 계획은 엔진이 받은 값에 맞춰 매번 새로 세웁니다. 안에 든 네이티브 쿼리는 가비지 컬렉터가 이 객체를 거둘 때 풀립니다.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');
final inAges = db.prepare(userSchema, r'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) {
  final users = txn.collection(userSchema);

  users.findOnePrepared(byEmail, ['alice@example.com']);
  users.findPrepared(inAges, [18, 30]);
});
```

`findText`, `findOneText`, `countText`에 바로 넘긴 문자열도 패키지가 준비해 둡니다. 데이터베이스마다 256개까지 기억하며, 문자열마다 실행한 컬렉션도 함께 기억합니다. 그래도 패키지가 기억하는 것보다 많은 문자열을 실행하는 프로그램에는 `prepare`가 도움이 됩니다.

- `prepare`는 해석할 수 없는 문자열이면 `INVALID_QUERY`, 데이터베이스를 닫은 뒤라면 `CLOSED`로 실패합니다.
- 다른 컬렉션에서 실행하면 `INVALID_ARGUMENT`로, 값을 받지 못한 매개변수가 있거나 쿼리의 필드나 값이 스키마와 맞지 않으면 `INVALID_QUERY`로 실행할 때 실패합니다.
