---
title: BackupReport
order: 9
group: tools
pageClass: reference-page
---

# BackupReport

`BackupReport` is what a backup wrote: the commit it copied, how much it copied, and the size of the copy.

```python
@dataclasses.dataclass(frozen=True)
class BackupReport:
    commit_id: int
    trees: int
    entries: int
    bytes: int
```

[`Database.backup` and `backup_async`](../../api/python/database.md#backup) return one once the copy is whole and durable at its path. The copy holds the published commit as it was when the backup began, while other handles and processes went on writing. [Tools](../../guide/tools.md) explains how a backup is made.

```python
report = db.backup("backups/app.darudb")  # or `await db.backup_async(path)`

print(f"{report.entries} entries of commit {report.commit_id}, {report.bytes} bytes")
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `commit_id` | `int` | The transaction id of the commit copied, the one published when the backup began |
| `trees` | `int` | The trees copied, the engine's own included |
| `entries` | `int` | The entries copied, those of the indexes included |
| `bytes` | `int` | The size of the new file, in bytes |
