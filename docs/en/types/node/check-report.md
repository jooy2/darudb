---
title: CheckReport
order: 10
---

# CheckReport

`CheckReport` is what the integrity check found: the commit it checked, how much it read, and every problem.

```ts
interface CheckReport
```

[`Database.check` and `checkAsync`](../../api/node/database.md) return one. The check reports a problem rather than throwing it, so damage shows up here with `ok` false, and the check throws only when it cannot begin, such as with `CLOSED`. [Tools](../../guide/tools.md) explains what the check reads.

```ts
const report = db.check(); // or `await db.checkAsync()`

if (!report.ok) {
  for (const { page, tree, message } of report.problems) {
    console.error(page, tree, message);
  }
}
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `ok` | `boolean` | Whether the check found nothing wrong: `problems` is empty |
| `commitId` | `number` | The transaction id of the commit checked, the one published when the check began |
| `pageCount` | `number` | The pages that commit counts, the header page included |
| `pagesChecked` | `number` | The pages read and verified |
| `objectsChecked` | `number` | The objects read and checked against their indexes |
| `problems` | `CheckProblem[]` | Every problem found, in the order found |

## CheckProblem

```ts
interface CheckProblem
```

One thing the check found wrong.

| Field | Type | Description |
| --- | --- | --- |
| `page` | `number \| null` | The page the problem is in, when it is in one page |
| `tree` | `string \| null` | The tree or the collection it was found in, when it was found in one |
| `message` | `string` | What is wrong |

`tree` is meant for a person to read. It holds a collection's name or a tree's, but for the engine's own trees it can hold a description such as `the free tree`, so a program should not look it up as a name.
