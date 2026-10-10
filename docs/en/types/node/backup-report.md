---
title: BackupReport
order: 11
group: tools
pageClass: reference-page
---

# BackupReport

`BackupReport` is what a backup wrote: the commit it copied, how much it copied, and the size of the copy.

```ts
interface BackupReport
```

[`Database.backup` and `backupAsync`](../../api/node/database.md) return one once the copy is whole and durable at its path. The copy holds the published commit as it was when the backup began, while other handles and processes went on writing. [Tools](../../guide/tools.md) explains how a backup is made.

```ts
const report = await db.backupAsync('backups/app.darudb'); // or `db.backup(path)`

console.log(`${report.entries} entries of commit ${report.commitId}, ${report.bytes} bytes`);
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `commitId` | `number` | The transaction id of the commit copied, the one published when the backup began |
| `trees` | `number` | The trees copied, the engine's own included |
| `entries` | `number` | The entries copied, those of the indexes included |
| `bytes` | `number` | The size of the new file, in bytes |
