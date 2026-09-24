---
title: Node.js
order: 4
---

# Node.js

Node.js 패키지는 Rust와 같은 엔진, 같은 파일을 씁니다. 스키마는 JavaScript로 선언하고, 트랜잭션은 함수 하나의 범위에서 돌며, 쿼리는 타입이 걸린 빌더나 문자열로 씁니다. 모든 메서드에 동기 버전과, 이벤트 루프를 막지 않는 비동기 버전이 있습니다.

## 스키마 선언하기

필드 타입은 `t`에 있고, `collection`으로 필드를 묶고, `schema`로 컬렉션에 버전을 붙입니다. 모든 객체의 TypeScript 타입이 이 선언에서 나옵니다.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  teams: collection({
    name: t.string().primaryKey(),
    city: t.string().optional()
  }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index(),
    tags: t.list(t.string()).optional().index(),
    team: t.link('teams').optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
  })
});

const db = Database.open('app.darudb', { schema: app });
```

- 타입은 `t.bool()`, `t.int()`, `t.bigint()`, `t.float()`, `t.string()`, `t.bytes()`, `t.link(collection)`, `t.list(type)`, `t.object(fields)`가 있습니다.
- `optional()`은 필드가 null일 수 있게 하고, `default(value)`는 필드가 빠졌을 때 채울 값을 정합니다. `index()`와 `unique()`는 인덱스를 두고, `primaryKey()`는 그 필드를 키로 삼습니다. 키 필드가 없는 컬렉션에는 엔진이 1부터 번호를 매기는 `id`가 생깁니다.
- `t.int()` 필드는 number입니다. number가 정확히 담지 못하는 2^53 너머의 값은 쓸 때 거부하고 읽을 때 실패합니다. 그런 값이 필요하면 `t.bigint()`로 선언하세요. 언제나 `bigint`로 읽힙니다. 바이트는 `Uint8Array`입니다.

엔진이 지키는 규칙은 언어와 관계없이 같고, [컬렉션과 객체](./objects.md)에 있습니다.

## 읽고 쓰기

`write`는 함수를 쓰기 트랜잭션 안에서 실행하고, 함수가 반환하면 커밋합니다. 함수가 예외를 던지면 그 안에서 한 일은 남지 않습니다. `read`는 함수를 읽기 트랜잭션 안에서 실행하고, 함수가 도는 동안 커밋 하나를 봅니다. 둘 다 함수가 반환한 값을 돌려주고, 트랜잭션이 함수보다 오래 남지 않습니다.

```ts
db.write((txn) => {
  txn.collection('teams').insert({ name: 'north', city: 'Seoul' });

  const users = txn.collection('users');

  users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31, team: 'north' },
    { name: 'Bob', tags: ['new'] }
  ]);
  users.put({ id: 2, name: 'Robert', age: 18 });
  users.delete(3);
});

const alice = db.read((txn) => txn.collection('users').get(1));
```

- `insert`와 `insertMany`는 키를 돌려줍니다. `put`과 `putMany`는 없으면 넣고 있으면 바꿉니다. `delete`는 지운 객체가 있었는지 돌려줍니다.
- 여러 객체를 한 번에 넘기면 버퍼 하나에 담아 엔진을 한 번만 부릅니다. 객체마다 부르는 것보다 훨씬 쌉니다.
- `insert`는 키나 고유 값이 이미 있으면 `DUPLICATE_KEY`로 실패합니다. 값의 타입이 틀리거나 스키마에 없는 속성이 있으면 `INVALID_ARGUMENT`로 실패합니다. 거부된 쓰기는 아무것도 바꾸지 않으므로 함수는 이어서 진행해도 됩니다.
- 트랜잭션은 동기입니다. promise를 돌려주는 함수는 거부하고, 그 트랜잭션은 취소합니다. 쓰기 트랜잭션은 겹칠 수 없습니다. 다른 쓰기 함수 안에서 `db.write`를 부르면 자기 자신을 기다리는 대신 곧바로 실패합니다. 다른 프로세스가 쓰고 있으면 5초, 또는 `busyTimeout` 옵션에 준 밀리초만큼 기다립니다.
- `db.write(fn, { durability: 'deferred' })`는 디스크를 기다리지 않고 반환합니다. 변경은 곧바로 읽기에 보이고, 다음 동기 커밋이나 `db.sync()`, `close`, 또는 1초 안에 디스크에 기록됩니다.

## 조회하기

`find`, `findOne`, `count`는 쿼리를 만드는 함수를 받습니다. `findOne`은 첫 객체에서 멈춥니다.

```ts
const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);

db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null));
  users.find((q) => q.where('tags', 'contains', 'new').where('age', 'between', [18, 30]));
  users.find((q) => q.where('team.city', '==', 'Seoul'));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
  users.count((q) => q.where('name', 'startsWith', 'A'));
});
```

TypeScript에서 `where`는 컬렉션에 있는 필드와 그 필드 타입의 값만 받습니다. `team.city`처럼 내장 객체나 링크를 지나는 경로는 쿼리를 실행할 때 엔진이 검사합니다. 같은 쿼리를 매개변수와 함께 문자열로 쓸 수도 있습니다.

```ts
users.find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [18, 'A']);
```

프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요.

## 비동기 API 쓰기

`Database`의 메서드마다 이름이 `Async`로 끝나는 짝이 있습니다. `openAsync`, `readAsync`, `writeAsync`, `syncAsync`, `closeAsync`입니다. 이 메서드들은 엔진의 일을 libuv 스레드 풀에서 하고 promise로 결과를 돌려줍니다. 엔진이 디스크나 다른 프로세스의 쓰기를 기다리는 동안에도 이벤트 루프는 계속 돕니다. 서버라면 이쪽을 쓰세요.

```ts
const db = await Database.openAsync('app.darudb', { schema: app });

const key = await db.writeAsync(async (txn) => {
  const users = txn.collection('users');
  const bob = await users.findOne((q) => q.where('name', '==', 'Bob'));

  if (bob !== null) {
    await users.put({ ...bob, age: bob.age + 1 });
  }

  return users.insert({ name: 'Carol' });
});

const adults = await db.readAsync((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18))
);
```

- 함수는 비동기여도 됩니다. `writeAsync`는 함수가 이행되면 커밋하고 거부되면 취소하며, `write`와 같은 `durability` 옵션을 받습니다. `readAsync`는 함수가 끝날 때까지 커밋 하나를 봅니다.
- 컬렉션의 메서드는 모두 promise를 돌려줍니다. 트랜잭션은 작업을 await 여부와 관계없이 부른 순서대로 실행하고, 마지막 작업이 끝난 뒤에야 커밋합니다. 거부된 작업은 동기 API에서처럼 아무것도 바꾸지 않습니다.
- 함께 부른 작업과, 앞선 작업이 스레드 풀에 가 있는 동안 부른 작업은 한 묶음으로 한 번에 엔진에 갑니다. 풀을 한 번 오가는 비용이 대부분의 작업보다 크므로, 하나씩 await하기보다 여러 개를 한꺼번에 시작하고 함께 기다리는 쪽이 훨씬 쌉니다. 예를 들면 `await Promise.all(keys.map((key) => users.get(key)))`입니다.
- 한 프로세스에서 같은 파일에 하는 쓰기는 `Database` 객체가 여러 개여도 차례로 실행됩니다. 두 번째 `writeAsync`는 스레드 풀의 스레드를 잡지 않고 첫 번째를 기다립니다. `syncAsync`와 `closeAsync`도 같은 줄에서 기다립니다. 아직 디스크에 기록되지 않은 미룬 커밋이 있으면 둘 다 쓰기가 끝나기를 기다려야 하기 때문입니다.
- 쓰기 트랜잭션은 여전히 겹칠 수 없습니다. `writeAsync` 함수 안에서 같은 파일에 `writeAsync`, `write`, `sync`, `close`를 부르면 `INVALID_ARGUMENT`로 실패하고, 각각의 비동기 버전도 마찬가지입니다. 비동기 쓰기가 진행 중일 때는 어디서 부르든 동기 `write`, `sync`, `close`가 같은 오류로 실패합니다. 기다리면 그 비동기 쓰기가 끝나는 데 필요한 이벤트 루프를 막기 때문입니다.
- `openAsync`는 마이그레이션 함수에 같은 비동기 컬렉션을 넘기고, 여기서는 `previous`와 `previousKeys`도 promise를 돌려줍니다. 마이그레이션 함수도 비동기일 수 있고, 각 단계는 함수가 부른 작업이 모두 끝나야 마무리됩니다.
- 스레드 풀의 스레드는 `UV_THREADPOOL_SIZE` 환경 변수로 바꾸지 않으면 네 개이고, Node.js의 파일 시스템 호출도 이 풀을 함께 씁니다.

## 마이그레이션하기

스키마가 바뀌면 버전을 올립니다. 새 컬렉션, 기본값이 있는 필드, 인덱스는 엔진이 알아서 추가합니다. 그 밖의 변경은 마이그레이션에 적고, 마이그레이션은 쓰기 트랜잭션 안에서 JavaScript 함수를 실행할 수 있습니다.

```ts
const app2 = schema(2, {
  teams: collection({ name: t.string().primaryKey(), city: t.string().optional() }),
  people: collection({
    fullName: t.string(),
    email: t.string().optional().unique(),
    age: t.string().default('')
  })
});

const db = Database.open('app.darudb', {
  schema: app2,
  migrations: [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      run(m) {
        const people = m.collection('people');

        for (const key of m.previousKeys('users')) {
          const before = m.previous('users', key);
          const person = people.get(key);

          if (before !== null && person !== null) {
            people.put({ ...person, age: `${before.age} years` });
          }
        }
      }
    }
  ]
});
```

`previous`는 마이그레이션 전 스키마대로 객체를 읽습니다. 예전 이름과 교체한 필드의 값까지 그대로 나오니, 객체를 쓰기 전에 이렇게 읽어 두세요. 함수가 예외를 던지면 파일은 예전 스키마와 데이터를 그대로 유지하고, `open`은 같은 예외를 던집니다.

## 오류

패키지가 던지는 모든 오류는 `code`가 엔진의 코드인 `Error`이고, 코드 목록은 [시작하기](./getting-started.md#오류)에 있습니다. 함수가 반환한 뒤에 트랜잭션이나 컬렉션을 쓰면 `CLOSED`가 납니다. 비동기 API에서는 promise가 이 오류로 거부됩니다.
