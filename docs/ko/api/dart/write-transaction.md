---
title: WriteTransaction
order: 7
---

# WriteTransaction

`WriteTransaction`은 데이터베이스의 변경을 모아 두었다가, `Database.write`가 실행하는 함수가 반환하면 한꺼번에 커밋합니다.

```dart
final class WriteTransaction extends ReadTransaction
```

`Database.write`가 하나를 시작해 함수에 넘기고, 함수가 반환하면 커밋하고 예외를 던지면 취소합니다. 그래서 예외를 던진 함수가 한 일은 하나도 남지 않습니다. 트랜잭션은 시작한 커밋과 자기 변경을 함께 읽습니다. `DUPLICATE_KEY`나 `INVALID_ARGUMENT`로 거부된 쓰기는 아무것도 바꾸지 않으므로, 함수는 계속 진행해 나머지를 커밋해도 됩니다. 트랜잭션과 거기서 얻은 컬렉션은 함수가 도는 동안에만 쓸 수 있고, 그 뒤에는 모든 호출이 `CLOSED`를 던집니다.

쓰기 트랜잭션은 모든 프로세스를 통틀어 파일마다 한 번에 하나입니다. 시작할 때 다른 프로세스의 쓰기를 `busyTimeout`만큼, 기본 5초 동안 기다리고, 그래도 끝나지 않으면 `BUSY`로 실패합니다. 한 isolate 안에서는 쓰기 트랜잭션이 겹칠 수 없습니다. 같은 파일에 대한 다른 쓰기의 함수 안에서 `write`를 부르면 자기 자신을 기다리는 대신 곧바로 `INVALID_ARGUMENT`로 실패합니다. 커밋은 `write`가 반환될 때 디스크에 있습니다. [Durability](../../types/dart/durability.md)로 지연 커밋을 고르면 그렇지 않습니다. 자세한 설명은 [트랜잭션](../../guide/transactions.md)에 있습니다.

```dart
db.write((txn) {
  final users = txn.collection(userSchema);
  final key = users.insert(const User(name: 'Alice', email: 'alice@example.com'));

  users.update(key, (q) => [q.age.set(31)]);
});
```

## 메서드

### collection

```dart
WriteCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> schema,
);
```

`schema`의 컬렉션을 [WriteCollection](./write-collection.md)으로 돌려줍니다. 트랜잭션의 변경을 읽고, 변경을 더합니다. 데이터베이스를 열 때 주지 않은 컬렉션이면 `INVALID_ARGUMENT`를 던집니다.

## AsyncWriteTransaction

```dart
final class AsyncWriteTransaction extends AsyncReadTransaction {
  AsyncWriteCollection<T, Q, K> collection<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
  );
}
```

`Database.writeAsync`의 쓰기 트랜잭션입니다. 함수는 비동기여도 됩니다. 함수가 완료되고 함수가 부른 호출이 모두 끝나면 커밋하고, 함수가 실패하면 취소합니다. `collection`은 위와 같고 [AsyncWriteCollection](./write-collection.md#asyncwritecollection)을 돌려줍니다. 그 호출은 await 여부와 관계없이 부른 순서대로 네이티브 라이브러리의 스레드에서 실행됩니다.

실패한 호출은 자기 `Future`를 그 오류로 완료하고 아무것도 바꾸지 않으며, 트랜잭션은 함수가 완료되면 나머지를 커밋합니다. 실패한 뒤에 커밋하면 안 되는 함수는 호출을 await해서 오류가 함수의 결과까지 올라가게 합니다. 함수 안에서 같은 파일에 `write`, `sync`, `close`, `compact`, `upgradeFormat`, `setKey`, `setPassword`를 부르면 `Async` 짝까지 모두 `INVALID_ARGUMENT`로 거부됩니다.

```dart
final carol = await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);
  final bob = await users.findOne((q) => q.where(q.name.equals('Bob')));

  if (bob != null) {
    await users.update(bob.id!, (q) => [q.age.set(bob.age + 1)]);
  }

  return users.insert(const User(name: 'Carol'));
});
```
