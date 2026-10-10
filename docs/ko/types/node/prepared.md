---
title: Prepared
order: 9
group: queries
pageClass: reference-page
---

# Prepared

`Prepared`는 컬렉션 하나에 대해 한 번 해석해 두고, 실행할 때마다 매개변수의 값을 주는 쿼리입니다.

```ts
interface Prepared<O> {
  readonly collection: string;
  readonly [objects]?: O;
}
```

[`Database.prepare`](../../api/node/database.md)가 만듭니다. `$0`, `$1` 같은 매개변수가 든 쿼리 언어 문자열로 만들 수도 있고, 바뀌는 값 자리에 [`param`](../../api/node/param.md)을 넣어 만든 쿼리로 만들 수도 있습니다. 다른 방법으로는 만들 수 없습니다. `find`, `findOne`, `count`가 이 쿼리와 함께 매개변수의 값을 [QueryParameters](./query-input.md#queryparameters)로 받습니다.

`O`는 쿼리가 찾는 객체의 타입이어서, 타입 검사기는 객체 타입이 다른 컬렉션에서 이 쿼리를 쓰지 못하게 합니다. `[objects]`는 `O`를 실어 나르는 속성으로, 타입 검사기만 알고 실행 중에는 없습니다.

준비한 쿼리는 데이터베이스도 트랜잭션도 붙잡고 있지 않습니다. 그래서 동기든 비동기든, 읽기든 쓰기든 어느 트랜잭션에서나 실행할 수 있습니다. 미리 준비해 두면 실행할 때마다 쿼리를 해석하거나 인코딩하는 비용이 빠지고, 문자열 쿼리에서 효과가 가장 큽니다. 실행 계획은 엔진이 받은 값에 맞춰 매번 새로 세웁니다.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const inAges = db.prepare('users', 'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) => {
  const users = txn.collection('users');

  users.findOne(byEmail, ['alice@example.com']);
  users.find(inAges, [18, 30]);
});
```

`find`, `findOne`, `count`에 바로 넘긴 문자열도 패키지가 준비해 둡니다. 4096자 이하인 문자열을 256개까지 기억하며, 문자열마다 실행한 컬렉션도 함께 기억합니다. 그래도 JavaScript로 만든 쿼리는 준비하지 않으면 실행할 때마다 인코딩하므로 `prepare`가 도움이 되고, 패키지가 기억하는 것보다 많은 문자열을 실행하는 프로그램에도 도움이 됩니다.

- `prepare`는 스키마에 없는 컬렉션이면 `INVALID_ARGUMENT`, 해석할 수 없는 문자열이면 `INVALID_QUERY`, 데이터베이스를 닫은 뒤라면 `CLOSED`로 실패합니다.
- 다른 컬렉션에서 실행하거나, 값을 받지 못한 매개변수가 있거나, 쿼리의 필드나 값이 스키마와 맞지 않으면 실행할 때 `INVALID_QUERY`로 실패합니다.

## 속성

### collection

```ts
readonly collection: string;
```

쿼리를 준비한 컬렉션이자, 쿼리를 실행할 수 있는 유일한 컬렉션입니다.
