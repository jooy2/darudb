---
title: CompactReport
order: 12
group: tools
pageClass: reference-page
---

# CompactReport

`CompactReport` is what compaction did: the size of the file before and after, and the pages it moved.

```ts
interface CompactReport
```

[`Database.compact` and `compactAsync`](../../api/node/database.md) return one. Compaction writes again, full, the trees whose pages inserts left part empty, then moves pages from the end of the file into free pages nearer its start, and gives the end back to the file system. It cannot move a page that a read transaction can still reach, so a long read transaction can leave `bytesAfter` close to `bytesBefore`, and a later compaction moves the rest. [Tools](../../guide/tools.md) explains when to compact.

```ts
const report = await db.compactAsync(); // or `db.compact()`

console.log(`${report.bytesBefore} bytes, then ${report.bytesAfter}`);
```

## Fields

| Field         | Type     | Description                           |
| ------------- | -------- | ------------------------------------- |
| `bytesBefore` | `number` | The size of the file before, in bytes |
| `bytesAfter`  | `number` | The size of the file after, in bytes  |
| `pagesMoved`  | `number` | The pages moved out of the file's end |
