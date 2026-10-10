---
title: SalvageReport
order: 13
group: tools
pageClass: reference-page
---

# SalvageReport

`SalvageReport`는 되살리기의 결과로, 새 파일에 건져 낸 것과 건지지 못한 것을 담습니다.

```ts
interface SalvageReport
```

[`Database.salvage`와 `salvageAsync`](../../api/node/database.md)가 새 파일을 다 만든 뒤 돌려줍니다. 잃은 것이 있는지는 `whole`이, 어디서 잃었는지는 나머지 수가 알려 줍니다. `whole`이 거짓이면 커밋에서 읽지 못한 부분을 같은 페이지의 옛 버전으로 채운 것입니다. 그래서 항목에 옛 값이 들어 있을 수 있고, 잃어버린 페이지에서 지워졌던 항목이 되살아날 수도 있습니다. 되살리기가 어떻게 동작하는지는 [도구](../../guide/tools.md)에 있습니다.

```ts
import { Database } from 'darudb';

const report = await Database.salvageAsync('app.darudb', 'rescued.darudb');

if (!report.whole) {
  console.warn(report.entriesRecovered, report.valuesLost, report.objectsDropped);
}
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `whole` | `boolean` | 새 파일이 되살리기를 시작한 커밋을 그대로 담는지. 그 커밋의 페이지를 모두 읽었고 버린 객체가 없으면 참입니다 |
| `commitId` | `number \| null` | 되살리기를 시작한 커밋의 트랜잭션 id. 쓸 수 있는 커밋 기록이 없어 모든 트리를 찾아낸 페이지로 만들었으면 `null`입니다 |
| `pagesScanned` | `number` | 읽은 파일 페이지 수. 헤더 페이지는 빼고 셉니다 |
| `pagesDamaged` | `number` | 검사를 통과하지 못한 페이지 수. 한 번도 쓰지 않은 페이지는 빼고 셉니다 |
| `pagesUnread` | `number` | 커밋에서 읽지 못한 페이지 수. 한 페이지에 다 들어가지 않는 값은 하나로 셉니다. 그 페이지에 있던 것은 같은 페이지의 옛 버전에서 가져왔습니다 |
| `entriesRecovered` | `number` | 그 옛 버전에서 가져온 항목 수 |
| `valuesLost` | `number` | 값을 어느 버전에서도 읽지 못해 뺀 키의 수 |
| `objectsDropped` | `number` | 뺀 객체 수. 읽을 수 없는 객체, 고유 인덱스 값을 다른 객체가 먼저 차지한 객체, 스키마를 잃은 파일의 모든 객체를 셉니다 |
| `trees` | `number` | 새 파일의 트리 수. 엔진 자체의 트리도 포함합니다 |
| `entries` | `number` | 새 파일의 항목 수. 인덱스의 항목도 포함합니다 |
| `bytes` | `number` | 새 파일의 크기(바이트) |
