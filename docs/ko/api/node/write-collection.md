---
title: WriteCollection
order: 8
counterpart: /api/rust/collection-writer
---

# WriteCollection

`WriteCollection`은 쓰기 트랜잭션 안에서 컬렉션 하나의 객체를 쓰고, `ReadCollection`처럼 읽기도 합니다.

```ts
interface WriteCollection<O, I> extends ReadCollection<O>
```

쓰기 트랜잭션이나 마이그레이션의 `collection`이 돌려줍니다. `O`는 읽은 객체의 타입이고, `I`는 쓰는 객체의 타입입니다. `I`는 [InsertOf](../../types/node/object-types.md)가 필드로 만들며, 기본값이 없는 필수 필드는 반드시 넣고 나머지는 넣어도 되고 빼도 됩니다. 읽기에는 트랜잭션 자신의 변경도 보입니다. 메서드는 트랜잭션의 함수가 도는 동안만 부를 수 있고, 그 뒤에 읽거나 쓰는 메서드는 `CLOSED`를 던집니다.

쓰기는 먼저 객체를 스키마에 맞춰 검사합니다.

- **`INVALID_ARGUMENT`**: 스키마에 맞지 않는 객체입니다. 타입이 다른 값, 빠진 필수 필드, 스키마에 없는 속성, 목록 안의 null, `t.int()`로 선언한 필드의 2^53 너머 값, UTF-8로 담을 수 없는 문자열이 그런 경우입니다. 값이 `undefined`나 `null`인 속성은 빠진 것으로 칩니다.
- **`DUPLICATE_KEY`**: 넣으려는 키가 이미 있거나, 고유 인덱스에 같은 값이 이미 있습니다.
- **거부된 쓰기는 아무것도 바꾸지 않으므로**, 트랜잭션은 이어서 진행하고 커밋해도 됩니다.

쓰기는 [Key](../../types/node/key.md)를 돌려줍니다. 정수 키는 number로, 2^53 너머면 `bigint`로 돌려주고, 문자열과 바이트 키는 그대로 돌려줍니다. 엔진이 지키는 규칙은 [객체](../../guide/objects.md)에 있습니다.

```ts
db.write((txn) => {
  const users = txn.collection('users');
  const [alice] = users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31 },
    { name: 'Bob', tags: ['new'] }
  ]);

  users.put({ id: 2, name: 'Robert', age: 18 });
  users.update(alice, { age: 32, email: null });
  users.delete(2);
});
```

## 메서드

### insert

```ts
insert(object: I): Key;
```

`object`를 넣고 기본 키를 돌려줍니다. 키 필드가 없는 컬렉션에서 `id` 없이 넣은 객체는 다음 번호를 받습니다.

### insertMany

```ts
insertMany(objects: readonly I[]): Key[];
```

`objects`를 엔진 호출 한 번으로 넣고, 키를 순서대로 돌려줍니다. 거부된 객체가 있으면 그 오류로 묶음이 멈추고, 앞서 넣은 객체는 트랜잭션에 남습니다. 함수가 예외를 던지지 않는 한 이 객체들은 커밋됩니다. 배열이 아닌 값을 주면 `INVALID_ARGUMENT`로 실패합니다. 묶음을 버퍼 하나에 인코딩해 엔진을 한 번만 부르므로, 객체마다 부르는 것보다 훨씬 쌉니다.

### put

```ts
put(object: I): Key;
```

`object`를 넣거나, 키가 같은 객체가 있으면 바꾸고, 키를 돌려줍니다. 키가 이미 있어도 실패하지 않는다는 점을 빼면 `insert`와 같은 경우에 실패합니다. 저장된 객체를 통째로 바꾸므로, `object`에서 빠진 필드에는 기본값이나 null이 들어갑니다.

### putMany

```ts
putMany(objects: readonly I[]): Key[];
```

객체마다 `put`을 하되 엔진은 한 번만 부르며, 묶음 규칙은 `insertMany`와 같습니다.

### update

```ts
update(key: Key, changes: Partial<I>): boolean;
```

기본 키가 `key`인 객체에서 `changes`에 있는 필드만 바꾸고, 객체가 있었는지 돌려줍니다. 객체가 없으면 아무것도 쓰지 않습니다. 나머지 필드는 그대로 둡니다.

- `null`을 주면 선택 필드는 null이 되고, 기본값이 있는 필드는 기본값이 됩니다. 기본값이 없는 필수 필드는 null로 바꿀 수 없습니다.
- 값이 `undefined`인 필드는 바뀌지 않습니다.
- 내장 객체와 목록은 통째로 바뀝니다.

거부되는 경우는 `put`과 같습니다. 컬렉션에 없는 필드, 타입이 다른 값, 객체가 아닌 `changes`는 `INVALID_ARGUMENT`로, 다른 객체가 가진 고유 값은 `DUPLICATE_KEY`로 실패합니다. 기본 키를 바꾸려 하면 `INVALID_ARGUMENT`로 실패하지만, 객체가 이미 가진 키를 넣는 것은 괜찮습니다. 바뀐 필드만 엔진으로 넘어가고 엔진이 레코드를 그 자리에서 고치므로, 객체를 읽어 `put`하는 것보다 쌉니다.

### delete

```ts
delete(key: Key): boolean;
```

기본 키가 `key`인 객체를 인덱스 항목과 함께 지우고, 객체가 있었는지 돌려줍니다. 정수, 문자열, 바이트가 아닌 키를 주면 `INVALID_ARGUMENT`로 실패합니다.

## AsyncWriteCollection

```ts
interface AsyncWriteCollection<O, I> extends AsyncReadCollection<O> {
  insert(object: I): Promise<Key>;
  insertMany(objects: readonly I[]): Promise<Key[]>;
  put(object: I): Promise<Key>;
  putMany(objects: readonly I[]): Promise<Key[]>;
  update(key: Key, changes: Partial<I>): Promise<boolean>;
  delete(key: Key): Promise<boolean>;
}
```

비동기 쓰기 트랜잭션의 컬렉션이며, [AsyncReadCollection](./read-collection.md#asyncreadcollection)의 읽기 메서드도 있습니다. 멤버가 하는 일은 위와 같고 결과를 promise로 돌려줍니다. 실패하면 같은 코드로 promise가 거부되고, 거부된 작업은 아무것도 바꾸지 않습니다. 작업은 await 여부와 관계없이 부른 순서대로 실행되고, 함께 부른 작업은 한 묶음으로 엔진에 갑니다. 한 작업이 실패해도 그 뒤에 부른 작업은 계속 실행됩니다.

```ts
await db.writeAsync(async (txn) => {
  const users = txn.collection('users');

  await Promise.all([
    users.insert({ name: 'Dave' }),
    users.update(1, { age: 33 }),
    users.delete(3)
  ]);
});
```
