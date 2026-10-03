---
title: 비동기 API
order: 11
languages: [node]
---

# 비동기 API

Node.js 패키지의 `Database` 메서드마다 이름이 `Async`로 끝나는 짝이 있고, 이 짝은 엔진의 일을 libuv 스레드 풀에서 하므로 이벤트 루프가 멈추지 않습니다.

## 쓰는 방법

`openAsync`, `readAsync`, `writeAsync`, `syncAsync`, `closeAsync`는 동기 버전과 같은 인자를 받고 promise로 결과를 돌려줍니다. 엔진이 디스크나 다른 프로세스의 쓰기를 기다리는 동안에도 이벤트 루프는 계속 돕니다. 서버라면 이쪽을 쓰세요.

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
- `readAsync`는 `read`처럼 부른 스레드에서 읽기를 시작합니다. 읽기를 시작할 때는 쓰기를 기다리지 않고, 스레드 풀을 한 번 오가는 것보다 비용이 적기 때문입니다.
- 도구에도 짝이 있습니다. `checkAsync`, `backupAsync`, `compactAsync`, `setKeyAsync`, `setPasswordAsync`, 그리고 `Database.salvageAsync`입니다.

## 작업이 실행되는 방식

- 컬렉션의 메서드는 모두 promise를 돌려줍니다. 트랜잭션은 작업을 await 여부와 관계없이 부른 순서대로 실행하고, 마지막 작업이 끝난 뒤에야 커밋합니다. 거부된 작업은 동기 API에서처럼 아무것도 바꾸지 않습니다.
- 함께 부른 작업과, 앞선 작업이 스레드 풀에 가 있는 동안 부른 작업은 한 묶음으로 한 번에 엔진에 갑니다. 풀을 한 번 오가는 비용이 대부분의 작업보다 크므로, 하나씩 await하기보다 여러 개를 한꺼번에 시작하고 함께 기다리는 쪽이 훨씬 쌉니다. 예를 들면 `await Promise.all(keys.map((key) => users.get(key)))`입니다.
- 스레드 풀의 스레드는 `UV_THREADPOOL_SIZE` 환경 변수로 바꾸지 않으면 네 개이고, Node.js의 파일 시스템 호출도 이 풀을 함께 씁니다.

## 쓰기는 차례로

- 한 프로세스에서 같은 파일에 하는 쓰기는 `Database` 객체가 여러 개여도 차례로 실행됩니다. 두 번째 `writeAsync`는 스레드 풀의 스레드를 잡지 않고 첫 번째를 기다립니다. `syncAsync`와 `closeAsync`도 같은 줄에서 기다립니다. 아직 디스크에 기록되지 않은 지연 커밋이 있으면 둘 다 쓰기가 끝나기를 기다려야 하기 때문입니다.
- 쓰기 트랜잭션은 여전히 겹칠 수 없습니다. `writeAsync` 함수 안에서 같은 파일에 `writeAsync`, `write`, `sync`, `close`를 부르면 `INVALID_ARGUMENT`로 실패하고, 각각의 비동기 버전도 마찬가지입니다.
- 비동기 쓰기가 진행 중일 때는 어디서 부르든 동기 `write`, `sync`, `close`가 같은 오류로 실패합니다. 기다리면 그 비동기 쓰기가 끝나는 데 필요한 이벤트 루프를 막기 때문입니다.

## 마이그레이션

`openAsync`는 마이그레이션 함수에 같은 비동기 컬렉션을 넘기고, 여기서는 `previous`와 `previousKeys`도 promise를 돌려줍니다. 마이그레이션 함수도 비동기일 수 있고, 각 단계는 함수가 부른 작업이 모두 끝나야 마무리됩니다.
