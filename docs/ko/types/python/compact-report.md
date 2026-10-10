---
title: CompactReport
order: 10
group: tools
pageClass: reference-page
---

# CompactReport

`CompactReport`는 압축의 결과로, 압축 전후의 파일 크기와 옮긴 페이지 수를 담습니다.

```python
@dataclasses.dataclass(frozen=True)
class CompactReport:
    bytes_before: int
    bytes_after: int
    pages_moved: int
```

[`Database.compact`와 `compact_async`](../../api/python/database.md#compact)가 돌려줍니다. 압축은 삽입으로 페이지가 덜 찬 트리를 꽉 채워 다시 쓴 다음, 파일 끝쪽 페이지를 앞쪽 빈 페이지로 옮기고 비게 된 끝을 파일 시스템에 돌려줍니다. 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 옮기지 못하므로, 오래 도는 읽기가 있으면 `bytes_after`가 `bytes_before`와 별 차이가 없을 수 있습니다. 나머지는 다음 압축이 옮깁니다. 언제 압축하면 좋은지는 [도구](../../guide/tools.md)에 있습니다.

```python
report = db.compact()  # 또는 `await db.compact_async()`

print(f"{report.bytes_before} bytes, then {report.bytes_after}")
```

## 필드

| 필드           | 타입  | 설명                                   |
| -------------- | ----- | -------------------------------------- |
| `bytes_before` | `int` | 압축 전 파일 크기. 단위는 바이트입니다 |
| `bytes_after`  | `int` | 압축 후 파일 크기. 단위는 바이트입니다 |
| `pages_moved`  | `int` | 파일 끝에서 옮긴 페이지 수             |
