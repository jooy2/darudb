---
title: SalvageReport
order: 13
---

# SalvageReport

`SalvageReport` is what salvage rescued into the new file, and what it could not.

```dart
final class SalvageReport
```

[`Database.salvage` and `salvageAsync`](../../api/dart/database.md#salvage) return one once the new file is complete. `whole` says whether anything was lost, and the counts say where. When `whole` is false, the parts of the commit salvage could not read were filled from older versions of the same pages: an entry may hold an older value, and an entry deleted in a lost page may be back. [Tools](../../guide/tools.md) explains how salvage works.

```dart
final report = await Database.salvageAsync('app.darudb', 'rescued.darudb');

if (!report.whole) {
  print('${report.entriesRecovered} ${report.valuesLost} ${report.objectsDropped}');
}
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `whole` | `bool` | Whether the new file holds exactly the commit salvage started from: every page of it was read, and no object was dropped |
| `commitId` | `int?` | The transaction id of the commit salvage started from, or `null` when no commit record could be used and every tree came from the pages found |
| `pagesScanned` | `int` | The pages of the file read, the header page left out |
| `pagesDamaged` | `int` | The pages that failed their check, other than pages never written |
| `pagesUnread` | `int` | The pages of the commit that could not be read, a value too large for one page counted as one. What they held was taken from older versions of the same pages |
| `entriesRecovered` | `int` | The entries taken from those older versions |
| `valuesLost` | `int` | The keys left out because no version of their value could be read |
| `objectsDropped` | `int` | The objects left out: those that could not be read, those whose value of a unique index another object had taken, and every object of a file whose schema was lost |
| `trees` | `int` | The trees of the new file, the engine's own included |
| `entries` | `int` | The entries of the new file, those of the indexes included |
| `bytes` | `int` | The size of the new file, in bytes |
