---
title: SalvageOptions
order: 14
group: tools
pageClass: reference-page
---

# SalvageOptions

`SalvageOptions`는 `Database.salvage`가 두 경로와 함께 받는 옵션으로, 암호화한 파일의 키나 비밀번호와 파일을 기다리는 시간을 정합니다.

```ts
interface SalvageOptions
```

[`Database.salvage`와 `salvageAsync`](../../api/node/database.md)가 마지막 인자로 받습니다. 필드는 모두 생략할 수 있습니다. 파일에 스키마가 들어 있으므로 스키마는 받지 않고, 암호화한 파일에는 해시 비용이 기록돼 있으므로 비용도 받지 않습니다.

```ts
import { Database } from 'darudb';

const report = Database.salvage('app.darudb', 'rescued.darudb', {
  password: 'correct horse battery staple',
  busyTimeout: 30_000
});
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `key` | `Uint8Array` | 암호화한 파일의 32바이트 키. 이 키로 새 파일도 열립니다 |
| `password` | `string \| Uint8Array` | 암호화한 파일의 비밀번호. 이 비밀번호로 새 파일도 열립니다 |
| `busyTimeout` | `number` | 다른 프로세스가 파일을 닫기를 기다리는 시간(밀리초). 이 시간이 지나면 `BUSY`로 실패하고, 기본값은 5000입니다 |

- 암호화한 파일을 키나 비밀번호 없이 되살리면 `KEY_REQUIRED`, 다른 키나 비밀번호로 되살리면 `WRONG_KEY`로 실패합니다.
- 암호화하지 않은 파일에 키나 비밀번호를 주거나, 32바이트가 아닌 키를 주거나, `key`와 `password`를 함께 주면 `INVALID_ARGUMENT`로 실패합니다.
- 패키지는 호출하는 순간 키나 비밀번호를 복사합니다. 그래서 넘긴 버퍼는 `salvage`가 반환하자마자 `fill(0)`으로 지워도 되고, `salvageAsync`라면 promise가 끝나기 전, 호출이 반환되자마자 지워도 됩니다.
