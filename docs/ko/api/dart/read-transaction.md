---
title: ReadTransaction
order: 6
---

# ReadTransaction

`ReadTransaction`은 `Database.read`가 실행하는 함수가 도는 동안 데이터베이스의 커밋 하나를 읽습니다.

```dart
base class ReadTransaction
```

`Database.read`가 하나를 시작해 함수에 넘기고, 함수가 반환하거나 예외를 던지면 끝냅니다. 읽는 내용은 모두 시작할 때 게시돼 있던 커밋에서 옵니다. 그 사이에 이 프로세스나 다른 프로세스가 한 커밋은 보이지 않습니다. 시작할 때 쓰기를 기다리지 않고, 쓰기도 이것을 기다리지 않습니다. 트랜잭션과 거기서 얻은 컬렉션은 함수가 도는 동안에만 쓸 수 있고, 그 뒤에는 모든 호출이 `CLOSED`를 던집니다.

다른 쪽이 쓰는 동안에는 읽기 트랜잭션을 짧게 유지하세요. 읽기 트랜잭션이 열려 있으면 이후 커밋이 더는 쓰지 않는 페이지를 재사용할 수 없어서, 쓰기가 새 페이지를 받고 파일이 커집니다. 자세한 설명은 [트랜잭션](../../guide/transactions.md)에 있습니다.

```dart
final names = db.read(
  (txn) => txn.collection(userSchema).find((q) => q.sortBy(q.name)).map((user) => user.name).toList(),
);
```

## 메서드

### collection

```dart
ReadCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> schema,
);
```

`schema`의 컬렉션을 [ReadCollection](./read-collection.md)으로 돌려줍니다. 객체는 클래스 `T`이고, 쿼리는 `Q`로 만들며, 기본 키는 `K`입니다. 타입 인자는 `userSchema` 같은 스키마 상수에서 나오므로 직접 적을 일이 없습니다. 데이터베이스를 열 때 주지 않은 컬렉션이면 `INVALID_ARGUMENT`를 던집니다. 이 핸들이 연 뒤에 다른 프로세스나 핸들이 파일을 마이그레이션했으면 컬렉션을 읽을 때 `SCHEMA_MISMATCH`로 실패하고, 새 스키마로 데이터베이스를 다시 열어야 합니다.

## AsyncReadTransaction

```dart
base class AsyncReadTransaction {
  AsyncReadCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
  );
}
```

`Database.readAsync`의 읽기 트랜잭션입니다. 함수는 비동기여도 되고, 트랜잭션은 함수가 끝날 때까지 커밋 하나를 봅니다. `collection`은 위와 같고 [AsyncReadCollection](./read-collection.md#asyncreadcollection)을 돌려줍니다. 그 호출은 `Future`를 돌려주며, 네이티브 라이브러리의 스레드에서 부른 순서대로 하나씩 실행됩니다. 트랜잭션은 함수가 끝나고 함수가 부른 호출이 모두 끝나면 끝나며, 그 뒤의 호출은 `CLOSED`로 실패합니다.

```dart
final (alice, adults) = await db.readAsync((txn) async {
  final users = txn.collection(userSchema);

  return (await users.get(1), await users.count((q) => q.where(q.age.atLeast(18))));
});
```
