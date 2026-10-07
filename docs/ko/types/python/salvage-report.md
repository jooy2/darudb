---
title: SalvageReport
order: 11
---

# SalvageReport

`SalvageReport`는 되살리기의 결과로, 새 파일에 건져 낸 것과 건지지 못한 것을 담습니다.

```python
@dataclasses.dataclass(frozen=True)
class SalvageReport:
    whole: bool
    commit_id: int | None
    pages_scanned: int
    pages_damaged: int
    pages_unread: int
    entries_recovered: int
    values_lost: int
    objects_dropped: int
    trees: int
    entries: int
    bytes: int
```

[`Database.salvage`와 `salvage_async`](../../api/python/database.md#salvage)가 새 파일을 다 만든 뒤 돌려줍니다. 잃은 것이 있는지는 `whole`이, 어디서 잃었는지는 나머지 수가 알려 줍니다. `whole`이 거짓이면 커밋에서 읽지 못한 부분을 같은 페이지의 옛 버전으로 채운 것입니다. 그래서 항목에 옛 값이 들어 있을 수 있고, 잃어버린 페이지에서 지워졌던 항목이 되살아날 수도 있습니다. 되살리기가 어떻게 동작하는지는 [도구](../../guide/tools.md)에 있습니다.

```python
import darudb

report = darudb.Database.salvage("app.darudb", "rescued.darudb")

if not report.whole:
    print(report.entries_recovered, report.values_lost, report.objects_dropped)
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `whole` | `bool` | 새 파일이 되살리기를 시작한 커밋을 그대로 담는지. 그 커밋의 페이지를 모두 읽었고 버린 객체가 없으면 참입니다 |
| `commit_id` | `int \| None` | 되살리기를 시작한 커밋의 트랜잭션 id. 쓸 수 있는 커밋 기록이 없어 모든 트리를 찾아낸 페이지로 만들었으면 `None`입니다 |
| `pages_scanned` | `int` | 읽은 파일 페이지 수. 헤더 페이지는 빼고 셉니다 |
| `pages_damaged` | `int` | 검사를 통과하지 못한 페이지 수. 한 번도 쓰지 않은 페이지는 빼고 셉니다 |
| `pages_unread` | `int` | 커밋에서 읽지 못한 페이지 수. 한 페이지에 다 들어가지 않는 값은 하나로 셉니다. 그 페이지에 있던 것은 같은 페이지의 옛 버전에서 가져왔습니다 |
| `entries_recovered` | `int` | 그 옛 버전에서 가져온 항목 수 |
| `values_lost` | `int` | 값을 어느 버전에서도 읽지 못해 뺀 키의 수 |
| `objects_dropped` | `int` | 뺀 객체 수. 읽을 수 없는 객체, 고유 인덱스 값을 다른 객체가 먼저 차지한 객체, 스키마를 잃은 파일의 모든 객체를 셉니다 |
| `trees` | `int` | 새 파일의 트리 수. 엔진 자체의 트리도 포함합니다 |
| `entries` | `int` | 새 파일의 항목 수. 인덱스의 항목도 포함합니다 |
| `bytes` | `int` | 새 파일의 크기. 단위는 바이트입니다 |
