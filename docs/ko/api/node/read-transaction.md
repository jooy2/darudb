---
title: ReadTransaction
order: 5
---

# ReadTransaction

`ReadTransaction`은 `Database.read`가 실행하는 함수가 도는 동안 데이터베이스의 커밋 하나를 읽습니다.

```ts
interface ReadTransaction<S>
```

`Database.read`가 읽기 트랜잭션을 시작해 함수에 넘기고, 함수가 반환하거나 예외를 던지면 끝냅니다. 읽는 내용은 모두 트랜잭션을 시작할 때 게시돼 있던 커밋에서 나옵니다. 그사이 이 프로세스나 다른 프로세스가 커밋한 내용은 보이지 않습니다. 시작할 때 쓰기를 기다리지 않고, 쓰기도 이 트랜잭션을 기다리지 않습니다. 트랜잭션과 트랜잭션이 준 컬렉션은 함수가 도는 동안만 쓸 수 있고, 그 뒤에 부르면 모두 `CLOSED`를 던집니다.

다른 쪽이 쓰는 파일에서는 읽기 트랜잭션을 짧게 유지하세요. 읽기 트랜잭션이 열려 있는 동안에는 이후 커밋이 더는 쓰지 않는 페이지를 다시 쓸 수 없어서, 쓰기가 새 페이지를 잡고 파일이 커집니다. `S`는 데이터베이스의 스키마이며, 컬렉션마다 객체의 타입을 정해 줍니다. 자세한 설명은 [트랜잭션](../../guide/transactions.md)에 있습니다.

```ts
const names = db.read((txn) =>
  txn
    .collection('users')
    .find((q) => q.sortBy('name'))
    .map((user) => user.name)
);
```

## 메서드

### collection

```ts
collection<N extends NameOf<S>>(name: N): ReadCollection<ObjectOf<FieldsOf<S, N>>>;
```

스키마의 `name` 컬렉션을 [ReadCollection](./read-collection.md)으로 돌려줍니다. `N`은 스키마에 있는 컬렉션 이름 중 하나이고, `FieldsOf<S, N>`은 그 컬렉션의 필드입니다. [ObjectOf](../../types/node/object-types.md)가 이 필드로 객체의 타입을 만듭니다. 스키마에 없는 이름이거나 스키마 없이 연 데이터베이스이면 `INVALID_ARGUMENT`를 던집니다. 이 핸들을 연 뒤에 다른 프로세스나 핸들이 파일을 마이그레이션했다면 컬렉션을 읽을 때 `SCHEMA_MISMATCH`로 실패하고, 새 스키마로 데이터베이스를 다시 열어야 합니다.

## AsyncReadTransaction

```ts
interface AsyncReadTransaction<S> {
  collection<N extends NameOf<S>>(name: N): AsyncReadCollection<ObjectOf<FieldsOf<S, N>>>;
}
```

`Database.readAsync`의 읽기 트랜잭션입니다. 함수는 비동기여도 되고, 트랜잭션은 함수가 끝날 때까지 커밋 하나를 봅니다. `collection`은 위와 같이 동작하되 [AsyncReadCollection](./read-collection.md#asyncreadcollection)을 돌려줍니다. 이 컬렉션의 작업은 promise를 돌려주고, 스레드 풀에서 부른 순서대로 하나씩 실행됩니다. 트랜잭션은 함수가 끝나고 함수가 부른 작업도 모두 끝나야 마무리되며, 그 뒤에 부른 작업은 `CLOSED`로 거부됩니다.

```ts
const [alice, adults] = await db.readAsync((txn) => {
  const users = txn.collection('users');

  return Promise.all([users.get(1), users.count((q) => q.where('age', '>=', 18))]);
});
```
