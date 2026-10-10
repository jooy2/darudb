---
title: CompactReport
order: 12
group: tools
pageClass: reference-page
---

# CompactReport

`CompactReport`는 압축의 결과로, 압축 전후의 파일 크기와 옮긴 페이지 수를 담습니다.

```dart
final class CompactReport
```

[`Database.compact`와 `compactAsync`](../../api/dart/database.md#compact)가 돌려줍니다. 압축은 삽입으로 페이지가 덜 찬 트리를 꽉 채워 다시 쓴 다음, 파일 끝쪽 페이지를 앞쪽 빈 페이지로 옮기고 비게 된 끝을 파일 시스템에 돌려줍니다. 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 옮기지 못하므로, 오래 도는 읽기가 있으면 `bytesAfter`가 `bytesBefore`와 별 차이가 없을 수 있습니다. 나머지는 다음 압축이 옮깁니다. 언제 압축하면 좋은지는 [도구](../../guide/tools.md)에 있습니다.

```dart
final report = await db.compactAsync(); // 또는 `db.compact()`

print('${report.bytesBefore} bytes, then ${report.bytesAfter}');
```

## 필드

| 필드          | 타입  | 설명                       |
| ------------- | ----- | -------------------------- |
| `bytesBefore` | `int` | 압축 전 파일 크기(바이트)  |
| `bytesAfter`  | `int` | 압축 후 파일 크기(바이트)  |
| `pagesMoved`  | `int` | 파일 끝에서 옮긴 페이지 수 |
