---
title: WriteTransaction
order: 6
---

# WriteTransaction

`WriteTransaction`은 데이터베이스에 대한 변경을 모아 두었다가, `Database.write`가 실행하는 함수가 반환할 때 한꺼번에 커밋합니다.

```ts
interface WriteTransaction<S>
```

`Database.write`가 쓰기 트랜잭션을 시작해 함수에 넘기고, 함수가 반환하면 커밋하고 예외를 던지면 취소합니다. 그래서 예외를 던진 함수가 한 일은 남지 않습니다. 트랜잭션은 시작할 때의 커밋에 자기 변경을 더한 상태를 읽습니다. `DUPLICATE_KEY`나 `INVALID_ARGUMENT`로 거부된 쓰기는 아무것도 바꾸지 않으므로, 함수는 이어서 진행하고 나머지를 커밋해도 됩니다. 트랜잭션과 그 컬렉션은 함수가 도는 동안만 쓸 수 있고, 그 뒤에 읽거나 쓰면 `CLOSED`를 던집니다.

한 파일에서는 모든 프로세스를 통틀어 한 번에 쓰기 트랜잭션 하나만 돕니다. 다른 프로세스가 쓰고 있으면 `busyTimeout` 밀리초(기본값 5000)까지 기다렸다가 `BUSY`로 실패합니다. 한 프로세스 안에서는 쓰기 트랜잭션이 겹칠 수 없습니다. 같은 파일에 대한 쓰기 함수 안에서 `write`를 부르면 자기 자신을 기다리는 대신 곧바로 `INVALID_ARGUMENT`로 실패합니다. [WriteOptions](../../types/node/write-options.md)로 미룬 커밋을 요청하지 않았다면, `write`가 반환할 때 커밋은 디스크에 기록돼 있습니다. `S`는 데이터베이스의 스키마입니다. 자세한 설명은 [트랜잭션](../../guide/transactions.md)에 있습니다.

```ts
db.write((txn) => {
  const users = txn.collection('users');
  const key = users.insert({ name: 'Alice', email: 'alice@example.com' });

  users.update(key, { age: 31 });
});
```

## 메서드

### collection

```ts
collection<N extends NameOf<S>>(
  name: N
): WriteCollection<ObjectOf<FieldsOf<S, N>>, InsertOf<FieldsOf<S, N>>>;
```

스키마의 `name` 컬렉션을 [WriteCollection](./write-collection.md)으로 돌려줍니다. `N`은 스키마에 있는 컬렉션 이름 중 하나이고, `FieldsOf<S, N>`은 그 컬렉션의 필드입니다. 읽은 객체의 타입은 [ObjectOf](../../types/node/object-types.md)가, 쓰는 객체의 타입은 `InsertOf`가 이 필드로 만듭니다. 스키마에 없는 이름이거나 스키마 없이 연 데이터베이스이면 `INVALID_ARGUMENT`를 던집니다.

## AsyncWriteTransaction

```ts
interface AsyncWriteTransaction<S> {
  collection<N extends NameOf<S>>(
    name: N
  ): AsyncWriteCollection<ObjectOf<FieldsOf<S, N>>, InsertOf<FieldsOf<S, N>>>;
}
```

`Database.writeAsync`의 쓰기 트랜잭션입니다. 함수는 비동기여도 됩니다. 함수의 promise가 이행되고 함수가 부른 작업이 모두 끝나면 커밋하고, promise가 거부되면 취소합니다. `collection`은 위와 같이 동작하되 [AsyncWriteCollection](./write-collection.md#asyncwritecollection)을 돌려줍니다. 이 컬렉션의 작업은 await 여부와 관계없이 부른 순서대로 스레드 풀에서 실행됩니다.

실패한 작업은 자기 promise를 거부하고 아무것도 바꾸지 않습니다. 함수의 promise가 이행되면 트랜잭션은 나머지를 그대로 커밋합니다. 실패한 뒤에 커밋하면 안 되는 함수라면 그 작업을 await해서 거부가 함수의 결과까지 올라가게 하세요. 함수 안에서 같은 파일에 `write`, `writeAsync`, `sync`, `close`, `compact`, `setKey`, `setPassword`나 그 짝 메서드를 부르면 `INVALID_ARGUMENT`로 거부됩니다.

```ts
const carol = await db.writeAsync(async (txn) => {
  const users = txn.collection('users');
  const bob = await users.findOne((q) => q.where('name', '==', 'Bob'));

  if (bob !== null) {
    await users.update(bob.id, { age: bob.age + 1 });
  }

  return users.insert({ name: 'Carol' });
});
```
