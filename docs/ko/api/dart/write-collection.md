---
title: WriteCollection
order: 9
counterpart: /api/rust/collection-writer
---

# WriteCollection

`WriteCollection`은 쓰기 트랜잭션 안에서 컬렉션 하나의 객체를 쓰고, `ReadCollection`처럼 읽기도 합니다.

```dart
final class WriteCollection<T, Q extends QueryBuilder<T>, K extends Object> extends ReadCollection<T, Q, K>
```

쓰기 트랜잭션이나 [MigrationContext](./migration-context.md)의 `collection`이 돌려줍니다. 읽기는 트랜잭션의 변경까지 봅니다. 메서드는 트랜잭션의 함수가 도는 동안에만 부를 수 있고, 그 뒤에는 `CLOSED`를 던집니다.

스키마에 맞지 않는 객체는 대부분 클래스의 타입이 막아서 컴파일되지 않습니다. 나머지는 객체를 쓸 때 검사합니다.

- **`INVALID_ARGUMENT`**: 파일의 스키마가 받지 않는 객체입니다. 링크에 대상 컬렉션과 다른 타입의 키가 들어 있거나, 다른 핸들의 마이그레이션이 바꾼 필드가 그 예입니다.
- **`DUPLICATE_KEY`**: 넣으려는 키가 이미 있거나, 고유 인덱스에 객체의 값이 이미 있습니다.
- **거부된 쓰기는 아무것도 바꾸지 않으므로**, 트랜잭션은 계속 진행해 커밋해도 됩니다.

엔진이 지키는 규칙은 [컬렉션과 객체](../../guide/objects.md)에 있습니다.

```dart
db.write((txn) {
  final users = txn.collection(userSchema);
  final [alice, _] = users.insertMany(const [
    User(name: 'Alice', email: 'alice@example.com', age: 31),
    User(name: 'Bob', tags: ['new']),
  ]);

  users.put(users.get(alice)!.copyWith(age: 32));
  users.update(alice, (q) => [q.email.set(null)]);
  users.delete(2);
});
```

## 메서드

### insert

```dart
K insert(T object);
```

`object`를 넣고 기본 키를 돌려줍니다. 자동 증가 키를 쓰는 컬렉션에서 `id`가 `null`인 객체는 다음 번호를 받고, `id`가 있으면 그 키로 들어갑니다.

### insertMany

```dart
List<K> insertMany(Iterable<T> objects);
```

`objects`를 엔진 호출 한 번으로 넣고 키를 순서대로 돌려줍니다. 거부된 객체가 있으면 묶음은 그 오류로 멈추고, 그 앞의 객체는 트랜잭션에 들어간 채로 남아 함수가 예외를 던지지 않으면 커밋됩니다. 묶음을 버퍼 하나에 담아 엔진을 한 번만 부르므로 객체마다 부르는 것보다 훨씬 쌉니다.

### put

```dart
K put(T object);
```

`object`를 넣거나, 키가 같은 객체를 바꾸고 키를 돌려줍니다. 키가 이미 있어도 실패하지 않는다는 점만 빼면 `insert`와 같은 경우에 실패합니다. 저장된 객체를 통째로 바꿉니다. 자동 증가 키를 쓰는 컬렉션에서 `id`가 `null`인 객체는 다음 번호로 들어갑니다.

### putMany

```dart
List<K> putMany(Iterable<T> objects);
```

객체마다 `put`을 엔진 호출 한 번으로 합니다. 묶음 규칙은 `insertMany`와 같습니다.

### update

```dart
bool update(K key, List<Change> Function(Q q) changes);
```

기본 키가 `key`인 객체에서 `changes`가 준 필드만 바꾸고, 객체가 있었는지 돌려줍니다. 객체가 없으면 아무것도 쓰지 않습니다. 함수는 쿼리 빌더를 받아, 필드의 `set`으로 만든 [Change](./fields.md#change)를 필드마다 돌려줍니다. 나머지 필드는 그대로입니다.

- `set(null)`을 주면 선택 필드는 null이 되고, 기본값이 있는 필드는 기본값이 됩니다. 기본값 없는 필수 필드는 null로 만들 수 없습니다.
- 목록은 통째로 바꿉니다. 내장 객체 필드에는 `set`이 없으므로, 내장 객체는 객체 전체를 `put`해서 바꿉니다.
- `set`은 객체 자신의 필드만 가리킵니다. 내장 객체나 링크를 거쳐 닿는 필드면 `INVALID_ARGUMENT`로 실패합니다.

```dart
users.update(alice, (q) => [q.age.set(37), q.email.set(null), q.tags.set(['admin'])]);
```

`put`과 같은 경우에 `INVALID_ARGUMENT`나 `DUPLICATE_KEY`로 거부됩니다. 기본 키를 바꾸려 하면 `INVALID_ARGUMENT`로 실패하고, 객체가 이미 가진 키를 주는 것은 괜찮습니다. 바꾼 필드만 엔진으로 넘어가고 엔진은 레코드를 그 자리에서 고치므로, 객체를 읽어 다시 넣는 것보다 쌉니다.

### delete

```dart
bool delete(K key);
```

기본 키가 `key`인 객체를 인덱스 항목과 함께 지우고, 객체가 있었는지 돌려줍니다.

## AsyncWriteCollection

```dart
final class AsyncWriteCollection<T, Q extends QueryBuilder<T>, K extends Object>
    extends AsyncReadCollection<T, Q, K> {
  Future<K> insert(T object);
  Future<List<K>> insertMany(Iterable<T> objects);
  Future<K> put(T object);
  Future<List<K>> putMany(Iterable<T> objects);
  Future<bool> update(K key, List<Change> Function(Q q) changes);
  Future<bool> delete(K key);
}
```

`Future` API 쓰기 트랜잭션의 컬렉션이며, [AsyncReadCollection](./read-collection.md#asyncreadcollection)의 읽기도 할 수 있습니다. 멤버는 위의 멤버와 같은 일을 하고 `Future`를 돌려줍니다. 실패하면 같은 오류로 `Future`를 완료하고, 거부된 호출은 아무것도 바꾸지 않습니다. 호출은 await 여부와 관계없이 부른 순서대로 실행되고, 실패한 호출이 있어도 뒤의 호출은 실행됩니다. 호출마다 라이브러리의 스레드를 한 번씩 오가므로, 객체 여럿은 `insertMany`나 `putMany` 한 번으로 넘기세요.

```dart
await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);

  await Future.wait([
    users.insert(const User(name: 'Dave')),
    users.update(1, (q) => [q.age.set(33)]),
    users.delete(3),
  ]);
});
```
