---
title: CheckReport
order: 10
group: tools
pageClass: reference-page
---

# CheckReport

`CheckReport`는 무결성 검사의 결과로, 검사한 커밋과 읽은 양, 찾은 문제를 모두 담습니다.

```dart
final class CheckReport
```

[`Database.check`와 `checkAsync`](../../api/dart/database.md#check)가 돌려줍니다. 검사는 문제를 예외로 던지지 않고 보고하므로 손상은 `ok`가 거짓인 보고서로 나타나며, `CLOSED`처럼 검사를 시작할 수 없을 때만 예외를 던집니다. 검사가 무엇을 읽는지는 [도구](../../guide/tools.md)에 있습니다.

```dart
final report = db.check(); // 또는 `await db.checkAsync()`

if (!report.ok) {
  for (final problem in report.problems) {
    print('${problem.page} ${problem.tree} ${problem.message}');
  }
}
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `ok` | `bool` | 검사에서 아무 문제도 찾지 못했는지. `problems`가 비어 있으면 참입니다 |
| `commitId` | `int` | 검사한 커밋의 트랜잭션 id. 검사를 시작할 때 게시돼 있던 커밋입니다 |
| `pageCount` | `int` | 그 커밋이 세는 페이지 수. 헤더 페이지도 포함합니다 |
| `pagesChecked` | `int` | 읽고 확인한 페이지 수 |
| `objectsChecked` | `int` | 읽어서 인덱스와 대조한 객체 수 |
| `problems` | `List<CheckProblem>` | 찾은 문제 전부. 찾은 순서대로입니다 |

## CheckProblem

```dart
final class CheckProblem
```

검사가 찾은 문제 하나입니다.

| 필드      | 타입      | 설명                                          |
| --------- | --------- | --------------------------------------------- |
| `page`    | `int?`    | 문제가 한 페이지 안에 있을 때 그 페이지       |
| `tree`    | `String?` | 문제를 한 트리에서 찾았을 때 그 트리나 컬렉션 |
| `message` | `String`  | 무엇이 잘못됐는지                             |

`tree`는 사람이 읽으라고 있는 값입니다. 컬렉션 이름이나 트리 이름이 들어가지만, 엔진 자체의 트리에는 `the free tree` 같은 설명이 들어갈 수 있습니다. 그러니 프로그램이 이 값을 이름으로 찾아 쓰면 안 됩니다.
