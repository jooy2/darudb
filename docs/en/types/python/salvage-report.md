---
title: SalvageReport
order: 11
group: tools
pageClass: reference-page
---

# SalvageReport

`SalvageReport` is what salvage rescued into the new file, and what it could not.

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

[`Database.salvage` and `salvage_async`](../../api/python/database.md#salvage) return one once the new file is complete. `whole` says whether anything was lost, and the counts say where. When `whole` is false, the parts of the commit salvage could not read were filled from older versions of the same pages: an entry may hold an older value, and an entry deleted in a lost page may be back. [Tools](../../guide/tools.md) explains how salvage works.

```python
import darudb

report = darudb.Database.salvage("app.darudb", "rescued.darudb")

if not report.whole:
    print(report.entries_recovered, report.values_lost, report.objects_dropped)
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `whole` | `bool` | Whether the new file holds exactly the commit salvage started from: every page of it was read, and no object was dropped |
| `commit_id` | `int \| None` | The transaction id of the commit salvage started from, or `None` when no commit record could be used and every tree came from the pages found |
| `pages_scanned` | `int` | The pages of the file read, the header page left out |
| `pages_damaged` | `int` | The pages that failed their check, other than pages never written |
| `pages_unread` | `int` | The pages of the commit that could not be read, a value too large for one page counted as one. What they held was taken from older versions of the same pages |
| `entries_recovered` | `int` | The entries taken from those older versions |
| `values_lost` | `int` | The keys left out because no version of their value could be read |
| `objects_dropped` | `int` | The objects left out: those that could not be read, those whose value of a unique index another object had taken, and every object of a file whose schema was lost |
| `trees` | `int` | The trees of the new file, the engine's own included |
| `entries` | `int` | The entries of the new file, those of the indexes included |
| `bytes` | `int` | The size of the new file, in bytes |
