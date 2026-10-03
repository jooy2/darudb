---
title: CheckReport
order: 10
---

# CheckReport

`CheckReport` is what the integrity check found: the commit it checked, how much it read, and every problem.

```dart
final class CheckReport
```

[`Database.check` and `checkAsync`](../../api/dart/database.md#check) return one. The check reports a problem rather than throwing it, so damage shows up here with `ok` false, and the check throws only when it cannot begin, such as with `CLOSED`. [Tools](../../guide/tools.md) explains what the check reads.

```dart
final report = db.check(); // or `await db.checkAsync()`

if (!report.ok) {
  for (final problem in report.problems) {
    print('${problem.page} ${problem.tree} ${problem.message}');
  }
}
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `ok` | `bool` | Whether the check found nothing wrong: `problems` is empty |
| `commitId` | `int` | The transaction id of the commit checked, the one published when the check began |
| `pageCount` | `int` | The pages that commit counts, the header page included |
| `pagesChecked` | `int` | The pages read and verified |
| `objectsChecked` | `int` | The objects read and checked against their indexes |
| `problems` | `List<CheckProblem>` | Every problem found, in the order found |

## CheckProblem

```dart
final class CheckProblem
```

One thing the check found wrong.

| Field     | Type      | Description                                                          |
| --------- | --------- | -------------------------------------------------------------------- |
| `page`    | `int?`    | The page the problem is in, when it is in one page                   |
| `tree`    | `String?` | The tree or the collection it was found in, when it was found in one |
| `message` | `String`  | What is wrong                                                        |

`tree` is meant for a person to read. It holds a collection's name or a tree's, but for the engine's own trees it can hold a description such as `the free tree`, so a program should not look it up as a name.
