---
title: Error
order: 15
counterpart: /types/rust/error
---

# Error

패키지가 던지는 오류는 모두 JavaScript `Error`이고, `message` 옆에 엔진의 오류 코드 가운데 하나인 `code`가 있습니다.

```ts
interface CodeError extends Error {
  code: string;
}
```

패키지 내부 코드는 던지는 오류를 이렇게 선언하지만, 이 타입도 오류 클래스도 내보내지 않습니다. 던지는 것은 `instanceof Error`도 참인 평범한 `Error`이고, `code` 속성이 따로 붙어 있습니다. strict 모드의 TypeScript는 잡은 값을 `unknown`으로 다루므로, 속성이 있는지 확인한 뒤에 읽습니다. 코드마다 언제 생기고 어떻게 대처하는지는 [오류](../../guide/errors.md)에 있습니다.

```ts
import { Database } from 'darudb';

try {
  Database.open('app.darudb', { create: false });
} catch (error) {
  if (error instanceof Error && 'code' in error && error.code === 'NOT_FOUND') {
    // 경로에 아무것도 없습니다.
  } else {
    throw error;
  }
}
```

## 속성

### code

```ts
code: string;
```

`DUPLICATE_KEY`처럼 `SCREAMING_SNAKE_CASE`로 쓴 실패 코드입니다. 모든 언어에서 같은 문자열이고, 한 번 릴리스한 코드는 이름을 바꾸지 않으므로 프로그램이 믿고 써도 됩니다.

### message

```ts
message: string;
```

무엇이 잘못됐는지 사람이 읽으라고 쓴 설명입니다. 메시지는 릴리스마다 문구가 바뀔 수 있으니, 메시지가 아니라 `code`로 비교하세요.

## 오류가 생기는 곳

- **엔진.** 엔진 안에서 난 실패는 엔진의 코드와 메시지를 그대로 담고 옵니다. 호출이 부른 스레드에서 돌았든 스레드 풀에서 돌았든 같습니다.
- **패키지의 검사.** 패키지는 받은 값이 엔진에 닿기 전에 먼저 검사하고, 실패하면 엔진의 코드를 씁니다. 옵션이나 객체, 키가 맞지 않으면 `INVALID_ARGUMENT`, 쿼리나 매개변수가 맞지 않으면 `INVALID_QUERY`, 파일에서 읽은 레코드를 해석할 수 없으면 `CORRUPTED`, 버그로만 생길 수 있는 일이면 `INTERNAL`입니다.
- **직접 넘긴 함수.** 트랜잭션 함수나 마이그레이션 함수가 던진 오류는 트랜잭션을 커밋하지 않고 끝낸 뒤 `read`, `write`, `open`과 그 `Async` 짝에서 던진 그대로 나옵니다. `code`는 원래 있던 것이 그대로 있고, 없는 경우가 많습니다.

## CLOSED

- `close`나 `closeAsync`를 부른 뒤에는 [Database](../../api/node/database.md)의 멤버 가운데 `path`, `isOpen`, `close`, `closeAsync`를 뺀 모든 것이 `CLOSED`를 던지고, 비동기 메서드는 이 오류로 거부됩니다. 다시 닫으면 아무 일도 일어나지 않습니다. `closeAsync`는 promise가 끝나기 전, 부르는 순간부터 새 작업을 받지 않습니다.
- 함수가 반환한 뒤에 트랜잭션이나 거기서 얻은 컬렉션을 쓰면 `CLOSED`를 던지고, 비동기 API에서는 이 오류로 거부됩니다. 트랜잭션 밖에서 쓸 것은 컬렉션이 아니라 읽은 객체로 남겨 두세요. 객체는 트랜잭션이 끝난 뒤에도 쓸 수 있는 평범한 값입니다.

## 비동기 호출

promise를 돌려주는 메서드는 예외를 던지지 않습니다. 인자를 거부할 때를 포함해 실패하면, 동기 버전이 던지는 것과 같은 `Error`로 promise가 거부됩니다. 비동기 트랜잭션에서 거부된 작업은 자기 promise만 거부하고 아무것도 바꾸지 않으며, 그 뒤에 부른 작업은 그대로 실행됩니다. 트랜잭션 함수가 그 거부를 밖으로 흘려보내면 트랜잭션은 취소되고 `writeAsync`가 같은 오류로 거부됩니다.

```ts
try {
  await db.writeAsync(async (txn) => {
    await txn.collection('users').insert({ name: 'Alice', email: 'alice@example.com' });
  });
} catch (error) {
  if (error instanceof Error && 'code' in error && error.code === 'DUPLICATE_KEY') {
    // 이미 쓰는 email입니다. 커밋된 것은 없습니다.
  } else {
    throw error;
  }
}
```
