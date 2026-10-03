---
title: BackupReport
order: 11
---

# BackupReport

`BackupReport` is what a backup wrote: the commit it copied, how much it copied, and the size of the copy.

```dart
final class BackupReport
```

[`Database.backup` and `backupAsync`](../../api/dart/database.md#backup) return one once the copy is whole and durable at its path. The copy holds the published commit as it was when the backup began, while other handles and processes went on writing. [Tools](../../guide/tools.md) explains how a backup is made.

```dart
final report = await db.backupAsync('backups/app.darudb'); // or `db.backup(path)`

print('${report.entries} entries of commit ${report.commitId}, ${report.bytes} bytes');
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `commitId` | `int` | The transaction id of the commit copied, the one published when the backup began |
| `trees` | `int` | The trees copied, the engine's own included |
| `entries` | `int` | The entries copied, those of the indexes included |
| `bytes` | `int` | The size of the new file, in bytes |
