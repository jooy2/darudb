---
title: CheckReport
order: 8
group: tools
pageClass: reference-page
---

# CheckReport

`CheckReport` is what the integrity check found: the commit it checked, how much it read, and every problem.

```python
@dataclasses.dataclass(frozen=True)
class CheckReport:
    ok: bool
    commit_id: int
    page_count: int
    pages_checked: int
    objects_checked: int
    problems: tuple[CheckProblem, ...]
```

[`Database.check` and `check_async`](../../api/python/database.md#check) return one. The check reports a problem rather than raising it, so damage shows up here with `ok` false, and the check raises only when it cannot begin, such as with `CLOSED`. [Tools](../../guide/tools.md) explains what the check reads.

```python
report = db.check()  # or `await db.check_async()`

if not report.ok:
    for problem in report.problems:
        print(problem.page, problem.tree, problem.message)
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `ok` | `bool` | Whether the check found nothing wrong: `problems` is empty |
| `commit_id` | `int` | The transaction id of the commit checked, the one published when the check began |
| `page_count` | `int` | The pages that commit counts, the header page included |
| `pages_checked` | `int` | The pages read and verified |
| `objects_checked` | `int` | The objects read and checked against their indexes |
| `problems` | `tuple[CheckProblem, ...]` | Every problem found, in the order found |

## CheckProblem

```python
@dataclasses.dataclass(frozen=True)
class CheckProblem:
    page: int | None
    tree: str | None
    message: str
```

One thing the check found wrong.

| Field     | Type          | Description                                                          |
| --------- | ------------- | -------------------------------------------------------------------- |
| `page`    | `int \| None` | The page the problem is in, when it is in one page                   |
| `tree`    | `str \| None` | The tree or the collection it was found in, when it was found in one |
| `message` | `str`         | What is wrong                                                        |

`tree` is meant for a person to read. It holds a collection's name or a tree's, but for the engine's own trees it can hold a description such as `the free tree`, so a program should not look it up as a name.
