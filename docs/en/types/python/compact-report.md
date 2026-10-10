---
title: CompactReport
order: 10
group: tools
pageClass: reference-page
---

# CompactReport

`CompactReport` is what compaction did: the size of the file before and after, and the pages it moved.

```python
@dataclasses.dataclass(frozen=True)
class CompactReport:
    bytes_before: int
    bytes_after: int
    pages_moved: int
```

[`Database.compact` and `compact_async`](../../api/python/database.md#compact) return one. Compaction moves pages from the end of the file into free pages nearer its start, and gives the end back to the file system. It cannot move a page that a read transaction can still reach, so a long read transaction can leave `bytes_after` close to `bytes_before`, and a later compaction moves the rest. [Tools](../../guide/tools.md) explains when to compact.

```python
report = db.compact()  # or `await db.compact_async()`

print(f"{report.bytes_before} bytes, then {report.bytes_after}")
```

## Fields

| Field          | Type  | Description                           |
| -------------- | ----- | ------------------------------------- |
| `bytes_before` | `int` | The size of the file before, in bytes |
| `bytes_after`  | `int` | The size of the file after, in bytes  |
| `pages_moved`  | `int` | The pages moved out of the file's end |
