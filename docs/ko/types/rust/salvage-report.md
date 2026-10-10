---
title: SalvageReport
order: 11
group: tools
pageClass: reference-page
---

# SalvageReport

`SalvageReport`는 되살리기가 손상된 파일에서 건진 것과 건지지 못한 것, 그리고 새로 쓴 파일의 크기를 담습니다.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SalvageReport
```

[`OpenOptions::salvage`](../../api/rust/open-options.md#salvage)가 새 파일을 온전하게 디스크에 기록한 뒤 돌려줍니다. 되살리기는 파일에 기록된 가장 새 커밋에서 시작하고, 그 커밋에서 읽지 못한 곳은 같은 페이지의 옛 버전으로 채웁니다. 아래 필드는 새 파일이 그 커밋과 얼마나 다른지 알려 줍니다. 되살리기가 파일을 어떻게 읽는지는 [도구](../../guide/tools.md)에 있습니다. 이 구조체에는 `#[non_exhaustive]`가 붙어 있어 릴리스에서 필드가 늘 수 있습니다. 필드는 이름으로 읽고, 구조 분해할 때는 `..`을 붙입니다.

```rust
use darudb::OpenOptions;

fn rescue() -> darudb::Result<()> {
    let report = OpenOptions::new().salvage("app.darudb", "rescued.darudb")?;

    if !report.is_whole() {
        eprintln!(
            "{} entries from older pages, {} values lost, {} objects dropped",
            report.entries_recovered, report.values_lost, report.objects_dropped
        );
    }

    Ok(())
}
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `commit_id` | `Option<u64>` | 되살리기를 시작한 커밋의 트랜잭션 id. 쓸 수 있는 커밋 기록이 없어 모든 트리를 훑어본 페이지에서 가져왔다면 `None`입니다 |
| `pages_scanned` | `u64` | 훑어본 파일의 페이지 수. 헤더 페이지는 빼고 셉니다 |
| `pages_damaged` | `u64` | 검사값이 맞지 않은 페이지 수. 한 번도 쓰지 않아 0으로 채워진 페이지는 세지 않습니다 |
| `pages_unread` | `u64` | 그 커밋에서 읽지 못한 페이지 수. 오버플로 값은 하나를 한 페이지로 셉니다. 그 아래 키는 파일에 같은 페이지의 옛 버전이 남아 있으면 거기서 가져왔습니다 |
| `entries_recovered` | `u64` | 그 옛 버전에서 가져온 항목 수 |
| `values_lost` | `u64` | 값을 어느 버전에서도 읽지 못해 뺀 키의 수 |
| `objects_dropped` | `u64` | 뺀 객체 수. 레코드를 해석할 수 없거나 자기 키가 아닌 키에 저장된 객체, 고유 인덱스 값을 다른 객체가 이미 차지한 객체, 저장된 스키마를 잃은 파일의 모든 객체가 여기에 들어갑니다 |
| `trees` | `u64` | 새 파일의 트리 수. 엔진 자체의 트리도 포함합니다 |
| `entries` | `u64` | 새 파일의 항목 수. 인덱스의 항목도 포함합니다 |
| `bytes` | `u64` | 새 파일의 크기(바이트) |

페이지의 옛 버전에서 가져온 항목은 옛 값을 가질 수 있고, 잃어버린 페이지에서 지워졌던 항목이 되살아날 수도 있습니다. 그래서 보고서가 온전하지 않다면 새 파일은 이 수만으로는 드러나지 않는 방식으로 커밋과 다를 수 있습니다.

## 메서드

### is_whole

```rust
pub fn is_whole(&self) -> bool
```

새 파일이 되살리기를 시작한 커밋을 그대로 담는지 알려 줍니다. 그런 커밋이 있었고(`commit_id`가 `Some`), 그 커밋의 페이지를 모두 읽었으며, 뺀 객체가 없어야 참입니다.
