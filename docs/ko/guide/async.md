---
title: 비동기 API
order: 13
languages: [node, dart, python]
---

# 비동기 API

`Database`에서 파일을 쓰는 메서드마다 이름이 <LangCode node="Async" dart="Async" python="_async" />로 끝나는 짝이 있고, 이 짝은 엔진의 일을 내 코드가 도는 스레드 밖에서 하므로 엔진이 기다리는 동안에도 코드가 계속 돕니다.

## 동기와 비동기 고르기

두 버전은 같은 일을 합니다. 다른 점은 그 일을 어느 스레드에서 하느냐이고, 비용도 여기서 갈립니다.

- **동기 버전은 호출 한 번의 비용이 적습니다.** 엔진이 부른 스레드에서 바로 일하고 결과를 돌려주므로, 다른 스레드를 오가는 비용이 없습니다. 키로 하는 조회나 객체 몇 개의 지연 커밋은 그렇게 한 번 오가는 시간보다 빨리 끝납니다.
- **동기 버전은 돌아올 때까지 부른 스레드를 붙잡습니다.** 그동안 그 스레드에서는 다른 코드가 돌지 않습니다.

대부분의 호출은 마이크로초 단위로 끝나지만, 스레드를 훨씬 오래 붙잡는 호출도 있습니다.

- 동기 커밋은 디스크를 기다립니다.
- 쓰기와 파일 열기는 다른 프로세스가 쓰고 있으면 그 쓰기를 기다립니다. 최대 대기 시간인 바쁨 대기 시간은 기본 5초입니다.
- 파일을 열 때 복구나 마이그레이션이 돌 수 있고, 암호화된 파일을 비밀번호로 열면 비밀번호를 키로 바꾸는 데 수십 밀리초가 걸립니다.
- 객체를 많이 돌려주는 쿼리는 그 객체를 다 읽는 만큼 걸리고, 검사, 백업, 압축, 되살리기 같은 도구는 파일 전체를 읽습니다.

::: lang node

- **서버나 Electron의 메인 프로세스**에서는 쓰기, 파일 열기, 도구에 비동기 버전을 쓰세요. 동기 호출이 기다리는 동안 서버는 다른 요청에 응답하지 못하고, Electron 앱의 창은 메인 프로세스에 보낸 요청의 답을 받지 못합니다. 키로 하는 조회와 작은 쿼리는 동기로 둬도 됩니다.
- **한 번에 한 가지 일만 하는 스크립트나 명령줄 도구**에서는 동기 버전을 쓰세요. 스레드를 기다리는 다른 일이 없으니 더 간단하고 빠릅니다.

:::

::: lang dart

- **Flutter 앱의 UI isolate**에서는 쓰기, 파일 열기, 도구에 `Future` 버전을 쓰세요. 동기 호출이 도는 동안 isolate는 프레임을 그리지 못합니다. 60Hz에서 프레임 하나에 주어진 시간은 16밀리초쯤인데, 비밀번호로 파일을 여는 데는 그보다 오래 걸리고, 다른 프로세스를 기다리는 쓰기는 몇 초가 걸릴 수도 있습니다. 키로 하는 조회와 작은 쿼리는 동기로 둬도 됩니다.
- **직접 띄운 백그라운드 isolate**에서는 무거운 일도 동기 버전으로 할 수 있고, UI는 멈추지 않습니다. 그 isolate는 [여러 프로세스](./processes.md)에 나온 대로 같은 파일에 `Database`를 따로 열고, 결과는 메시지로 UI isolate에 보냅니다.
- **Dart 서버나 명령줄 도구**에서는 요청 여럿을 동시에 처리하는 서버라면 `Future` 버전을, 한 번에 한 가지 일만 하는 도구라면 동기 버전을 쓰세요.

:::

::: lang python

- **서버처럼 `asyncio` 이벤트 루프에서 도는 프로그램**에서는 쓰기, 파일 열기, 도구에 비동기 버전을 쓰세요. 동기 호출이 기다리는 동안 루프는 다른 태스크를 돌리지 못하므로, 서버는 다른 요청에 응답하지 못합니다. 키로 하는 조회와 작은 쿼리는 동기로 둬도 됩니다.
- **스크립트나 명령줄 도구, 스레드를 쓰는 프로그램**에서는 동기 버전을 쓰세요. 엔진이 일하는 동안 네이티브 모듈이 GIL을 놓으므로, 한 스레드가 디스크나 다른 쓰기를 기다리는 동안에도 프로그램의 다른 스레드는 계속 돕니다.

:::

한 프로세스에서 같은 파일에 두 버전을 섞어 써도 됩니다. 다만 비동기 쓰기가 파일을 쥐고 있는 동안에는 동기 쓰기가 거부되며, 자세한 내용은 [쓰기는 차례로](#쓰기는-차례로)에 있습니다.

## 쓰는 방법

::: lang node

`openAsync`, `readAsync`, `writeAsync`, `syncAsync`, `closeAsync`는 동기 버전과 같은 인자를 받고 promise로 결과를 돌려줍니다. 엔진의 일은 libuv 스레드 풀에서 하므로, 엔진이 디스크나 다른 프로세스의 쓰기를 기다리는 동안에도 이벤트 루프는 계속 돕니다. 서버라면 이쪽을 쓰세요.

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
- 도구에도 짝이 있습니다. `checkAsync`, `backupAsync`, `compactAsync`, `upgradeFormatAsync`, `setKeyAsync`, `setPasswordAsync`, 그리고 `Database.salvageAsync`입니다.

:::

::: lang dart

`openAsync`, `readAsync`, `writeAsync`, `syncAsync`, `closeAsync`는 동기 버전과 같은 인자를 받고 `Future`를 돌려줍니다. 엔진의 일은 패키지의 네이티브 라이브러리가 가진 스레드에서 하고, 결과는 isolate의 이벤트 루프로 돌아옵니다. 그래서 Flutter 앱의 UI isolate는 디스크도, 다른 쓰기도 기다리지 않습니다.

```dart
final db = await Database.openAsync('app.darudb', schema: const Schema(1, [userSchema]));

final key = await db.writeAsync((txn) async {
  final users = txn.collection(userSchema);
  final bob = await users.findOne((q) => q.where(q.name.equals('Bob')));

  if (bob != null) {
    await users.put(bob.copyWith(age: bob.age + 1));
  }

  return users.insert(const User(name: 'Carol'));
});

final adults = await db.readAsync(
  (txn) => txn.collection(userSchema).find((q) => q.where(q.age.atLeast(18))),
);
```

- 함수는 비동기여도 됩니다. `writeAsync`는 함수가 완료되면 커밋하고 실패하면 취소하며, `write`와 같은 `durability`를 받습니다. `readAsync`는 함수가 끝날 때까지 커밋 하나를 봅니다.
- `readAsync`는 `read`처럼 부른 isolate에서 읽기를 시작합니다. 읽기를 시작할 때는 쓰기를 기다리지 않기 때문입니다.
- 도구에도 짝이 있습니다. `checkAsync`, `backupAsync`, `compactAsync`, `upgradeFormatAsync`, `setKeyAsync`, `setPasswordAsync`, 그리고 `Database.salvageAsync`입니다.

:::

::: lang python

`Database.open_async`와 `sync_async`, `close_async`는 동기 버전과 같은 인자를 받고 await로 기다립니다. `db.read_async()`와 `db.write_async()`는 `async with`로 쓰며, 그 안에서는 컬렉션의 메서드를 모두 await합니다. 엔진의 일은 패키지가 따로 두는 스레드 풀에서 하고, 네이티브 모듈은 거기서 GIL을 놓습니다. 그래서 엔진이 디스크나 다른 프로세스의 쓰기를 기다리는 동안에도 이벤트 루프는 계속 돕니다.

```python
import darudb
from darudb import F

db = await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User]))

async with db.write_async() as txn:
    users = txn.collection(User)
    bob = await users.find_one(F.name == "Bob")

    if bob is not None:
        await users.update(bob.id, age=bob.age + 1)

    key = await users.insert(User(name="Carol"))

async with db.read_async() as txn:
    adults = await txn.collection(User).find(F.age >= 18)

await db.close_async()
```

- `write_async` 블록은 끝날 때, 그 안에서 시작한 작업이 모두 끝난 뒤에 커밋하고, 예외가 나면 취소합니다. `write`와 같은 `durability`를 받습니다. `read_async` 블록은 끝날 때까지 커밋 하나를 봅니다.
- `read_async`는 `read`처럼 이벤트 루프의 스레드에서 읽기를 시작합니다. 읽기를 시작할 때는 쓰기를 기다리지 않기 때문입니다. `write_async`는 쓰기를 기다려야 하므로 풀에서 시작합니다.
- 데이터베이스는 비동기 컨텍스트 관리자이기도 합니다. `async with await darudb.Database.open_async(...) as db:`로 열면 블록이 끝날 때 `close_async`로 닫힙니다.
- 도구에도 짝이 있습니다. `check_async`, `backup_async`, `compact_async`, `upgrade_format_async`, `set_key_async`, `set_password_async`, 그리고 `Database.salvage_async`입니다.

:::

## 작업이 실행되는 방식

::: lang node

- 컬렉션의 메서드는 모두 promise를 돌려줍니다. 트랜잭션은 작업을 await 여부와 관계없이 부른 순서대로 실행하고, 마지막 작업이 끝난 뒤에야 커밋합니다. 거부된 작업은 동기 API에서처럼 아무것도 바꾸지 않습니다.
- 함께 부른 작업과, 앞선 작업이 스레드 풀에 가 있는 동안 부른 작업은 한 묶음으로 한 번에 엔진에 갑니다. 풀을 한 번 오가는 비용이 대부분의 작업보다 크므로, 하나씩 await하기보다 여러 개를 한꺼번에 시작하고 함께 기다리는 쪽이 훨씬 쌉니다. 예를 들면 `await Promise.all(keys.map((key) => users.get(key)))`입니다.
- 스레드 풀의 스레드는 `UV_THREADPOOL_SIZE` 환경 변수로 바꾸지 않으면 네 개이고, Node.js의 파일 시스템 호출도 이 풀을 함께 씁니다.

:::

::: lang dart

- 컬렉션의 메서드는 모두 `Future`를 돌려줍니다. 트랜잭션은 작업을 await 여부와 관계없이 부른 순서대로 실행하고, 마지막 작업이 끝난 뒤에야 커밋합니다. 실패한 작업은 동기 API에서처럼 아무것도 바꾸지 않습니다.
- 작업마다 라이브러리의 스레드를 한 번 오가고, 이 비용이 대부분의 작업보다 큽니다. `insertMany` 같은 묶음 작업은 객체가 몇 개든 한 번만 오갑니다.
- 라이브러리의 스레드는 모두 바쁘면 늘어납니다. 그래서 다른 쓰기를 기다리는 쓰기가, 그 쓰기에 필요한 마지막 스레드를 잡고 있는 일은 없습니다. 10초 동안 할 일이 없는 스레드는 끝납니다.

:::

::: lang python

- 컬렉션의 메서드는 모두 코루틴입니다. 트랜잭션은 작업을 시작한 순서대로 하나씩 실행하므로, `insert` 여러 개를 `asyncio.gather`로 함께 기다리면 넘긴 순서대로 실행됩니다. 거부된 작업은 동기 API에서처럼 아무것도 바꾸지 않습니다.
- 작업마다 풀의 스레드를 한 번 오가고, 이 비용이 대부분의 작업보다 큽니다. `insert_many` 같은 묶음 작업은 객체가 몇 개든 한 번만 오갑니다. 함께 시작한 작업을 묶어 주지는 않으므로, 작업마다 따로 하나씩 차례로 오갑니다.
- 이 풀은 이벤트 루프의 기본 실행기가 아니라 패키지가 따로 둔 풀입니다. 스레드가 모두 바쁠 때만 새 스레드를 만들고, 32개와 프로세서 수에 4를 더한 수 가운데 작은 쪽까지 늘어납니다.

:::

## 쓰기는 차례로

::: lang node

- 한 프로세스에서 같은 파일에 하는 쓰기는 `Database` 객체가 여러 개여도 차례로 실행됩니다. 두 번째 `writeAsync`는 스레드 풀의 스레드를 잡지 않고 첫 번째를 기다립니다. `syncAsync`와 `closeAsync`도 같은 줄에서 기다립니다. 아직 디스크에 기록되지 않은 지연 커밋이 있으면 둘 다 쓰기가 끝나기를 기다려야 하기 때문입니다. 파일에 쓰는 `compactAsync`, `upgradeFormatAsync`, `setKeyAsync`, `setPasswordAsync`도 같은 줄에서 기다립니다.
- 쓰기 트랜잭션은 여전히 겹칠 수 없습니다. `writeAsync` 함수 안에서 같은 파일에 `writeAsync`, `write`, `sync`, `close`, `compact`, `upgradeFormat`, `setKey`, `setPassword`를 부르면 `INVALID_ARGUMENT`로 실패하고, 각각의 비동기 버전도 마찬가지입니다.
- 같은 파일에 비동기 쓰기가 진행 중일 때는 어디서 부르든 동기 `write`, `sync`, `close`, `compact`, `upgradeFormat`, `setKey`, `setPassword`가 같은 오류로 실패합니다. 기다리면 그 비동기 쓰기가 끝나는 데 필요한 이벤트 루프를 막기 때문입니다.

:::

::: lang dart

- 한 isolate에서 같은 파일에 하는 비동기 쓰기는 `Database` 객체가 여러 개여도 차례로 실행됩니다. 두 번째 `writeAsync`는 첫 번째를 기다리고, `syncAsync`, `closeAsync`, `compactAsync`, `upgradeFormatAsync`와 키 변경도 쓰기를 기다려야 하므로 같은 줄에서 기다립니다.
- 쓰기 트랜잭션은 여전히 겹칠 수 없습니다. `writeAsync` 함수 안에서 같은 파일에 쓰기, 동기화, 닫기, 압축, 형식 올리기, 키 변경을 하면 동기든 비동기든 `INVALID_ARGUMENT`로 실패합니다.
- 같은 파일에 비동기 쓰기가 진행 중일 때는 동기 `write`, `sync`, `close`, `compact`, `upgradeFormat`, `setKey`, `setPassword`가 같은 오류로 실패합니다. 기다리면 그 비동기 쓰기가 끝나는 데 필요한 isolate를 붙잡기 때문입니다.
- 다른 isolate의 쓰기는 다른 줄입니다. 다른 프로세스의 쓰기처럼 엔진 안에서 이 isolate의 쓰기를 기다립니다.

:::

::: lang python

- 한 이벤트 루프에서 같은 파일에 하는 비동기 쓰기는 `Database` 객체가 여러 개여도 차례로 실행됩니다. 두 번째 `write_async`는 풀의 스레드를 잡지 않고 이벤트 루프에서 첫 번째를 기다립니다. `sync_async`, `close_async`, `compact_async`, `upgrade_format_async`와 키 변경도 쓰기를 기다려야 하므로 같은 줄에서 기다립니다.
- 쓰기 트랜잭션은 여전히 겹칠 수 없습니다. 동기 `write` 블록 안에서 같은 파일에 `write_async`를 쓰면 `INVALID_ARGUMENT`로 실패합니다. `write_async` 블록 안에서 같은 파일에 `write_async`, `sync_async`, `close_async`, `compact_async`, `upgrade_format_async`나 키 변경을 해도 같은 오류로 실패합니다. 저마다 블록이 끝난 다음에야 차례가 오는데 블록은 그 호출이 끝나기를 기다리므로, 결국 자기 자신을 기다리게 되기 때문입니다. 블록 안에서 만든 태스크도 블록 안으로 칩니다.
- 같은 파일에 비동기 쓰기가 진행 중일 때 이벤트 루프의 스레드에서 동기 `write`, `sync`, `close`, `compact`, `upgrade_format`, `set_key`, `set_password`를 부르면 `INVALID_ARGUMENT`로 실패합니다. 기다리면 그 비동기 쓰기가 끝나는 데 필요한 스레드를 붙잡기 때문입니다. 다른 스레드에서 부르면 다른 프로세스의 쓰기처럼 쓰기가 끝나기를 기다립니다.
- 다른 스레드에서 도는 다른 이벤트 루프의 쓰기는 다른 줄입니다. 다른 프로세스의 쓰기처럼 엔진 안에서 이 루프의 쓰기를 기다립니다.

:::

## 마이그레이션

::: lang node

`openAsync`는 마이그레이션 함수에 같은 비동기 컬렉션을 넘기고, 여기서는 `previous`와 `previousKeys`도 promise를 돌려줍니다. 마이그레이션 함수도 비동기일 수 있고, 각 단계는 함수가 부른 작업이 모두 끝나야 마무리됩니다.

:::

::: lang dart

`openAsync`에서는 마이그레이션 함수가 비동기여도 되고, 각 단계는 함수의 `Future`가 완료되면 마무리됩니다. 함수가 `MigrationContext`에서 부르는 메서드는 `open`에서처럼 동기입니다. 마이그레이션이 쓰기를 쥐고 있으므로, 그 메서드가 다른 쓰기를 기다릴 일이 없습니다.

:::

::: lang python

`open_async`에서는 마이그레이션의 `run`이 코루틴 함수여도 됩니다. 이 함수가 받는 `AsyncMigrating`은 컬렉션이 비동기이고, `previous`와 `previous_keys`도 await로 기다립니다. 각 단계는 함수와, 함수가 시작한 작업이 모두 끝나야 마무리됩니다. `open`에 코루틴 함수를 주면 `INVALID_ARGUMENT`로 실패합니다.

:::
