---
title: param
order: 11
---

# param

`param`은 `Database.prepare`로 준비할 쿼리에서 값 자리에 매개변수를 넣어, 쿼리를 실행할 때마다 그 값을 받게 합니다.

```ts
const param: (index: number) => Param;
```

`param(0)`은 첫 매개변수로 쿼리 언어의 `$0`과 같고, `param(1)`은 두 번째 매개변수입니다. 매개변수는 조건이 비교하는 값이면 어디든 들어갈 수 있습니다. `where`의 값, `between`의 양 끝, `in`의 원소가 그렇습니다. 필드, 연산자, 정렬, 개수 제한, 오프셋 자리에는 쓸 수 없습니다. `index`는 0 이상의 정수여야 하고, 아니면 `param`이 `INVALID_QUERY`를 던집니다.

매개변수가 든 쿼리는 [Database.prepare](./database.md#prepare)로 준비해야 합니다. 준비하지 않고 실행하면 `INVALID_QUERY`로 실패합니다. 준비한 쿼리인 [Prepared](../../types/node/prepared.md)를 `find`, `findOne`, `count`로 실행할 때마다 [QueryParameters](../../types/node/query-input.md) 배열에 값을 순서대로 넘깁니다. 값을 받지 못한 매개변수가 있거나 값의 타입이 필드와 다르면 `INVALID_QUERY`로 실패합니다. `==`나 `!=`의 매개변수에 `null`을 넘기면, 쿼리에 `null`을 직접 쓴 것처럼 필드가 null인지 검사합니다.

준비해 두면 실행할 때마다 쿼리를 만들고 인코딩하고 해석하는 비용이 빠집니다. 실행 계획은 엔진이 넘겨받은 값에 맞춰 매번 새로 세웁니다. 준비한 쿼리는 준비할 때 정한 컬렉션에서만 실행되며, 동기든 비동기든, 읽기든 쓰기든 어느 트랜잭션에서나 쓸 수 있습니다.

```ts
import { param } from 'darudb';

const inAges = db.prepare('users', (q) =>
  q.where('age', 'between', [param(0), param(1)]).sortBy('age')
);
const byEmail = db.prepare('users', 'email == $0');

db.read((txn) => {
  const users = txn.collection('users');

  users.find(inAges, [18, 30]);
  users.findOne(byEmail, ['alice@example.com']);
  users.count(byEmail, [null]); // 이메일이 없는 사용자 수
});
```

`param`은 동결된 `Param`을 돌려줍니다.

```ts
interface Param
```

## 속성

### index

```ts
readonly index: number;
```

매개변수의 번호이며 0부터 셉니다.
