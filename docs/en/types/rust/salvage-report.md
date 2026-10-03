---
title: SalvageReport
order: 8
---

# SalvageReport

`SalvageReport` is what a salvage rescued from a damaged file and what it could not, with the size of the new file it wrote.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SalvageReport
```

[`OpenOptions::salvage`](../../api/rust/open-options.md#salvage) returns one once the new file is whole and durable. Salvage starts from the newest commit the file records and fills what that commit cannot read from older versions of the same pages, so the fields below say how far the new file is from that commit. [Tools](../../guide/tools.md) explains how salvage reads a file. The struct is `#[non_exhaustive]`, so fields may be added in a release: read the fields by name, and use `..` when destructuring one.

```rust
use darudb::OpenOptions;

fn rescue() -> darudb::Result<()> {
    let report = OpenOptions::new().salvage("app.darudb", "rescued.darudb")?;

    if !report.is_whole() {
        eprintln!(
            "{} entries from older pages, {} values lost, {} objects dropped",
            report.entries_recovered, report.values_lost, report.objects_dropped
        );
    }

    Ok(())
}
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `commit_id` | `Option<u64>` | The transaction id of the commit salvage started from, or `None` when no commit record could be used and every tree came from the pages the scan found |
| `pages_scanned` | `u64` | The pages of the file the scan read, the header page left out |
| `pages_damaged` | `u64` | The pages that failed their check, not counting pages of zeros, which were never written |
| `pages_unread` | `u64` | The pages of the commit that could not be read, each overflow value counted as one page; the keys under them were taken from older versions of the same pages where the file still had them |
| `entries_recovered` | `u64` | The entries taken from those older versions |
| `values_lost` | `u64` | The keys left out because no version of their value could be read |
| `objects_dropped` | `u64` | The objects left out: records that did not decode or were stored under another key than their own, objects whose value of a unique index another object had taken, and every object of a file whose stored schema was lost |
| `trees` | `u64` | The trees of the new file, the engine's own included |
| `entries` | `u64` | The entries of the new file, the indexes' included |
| `bytes` | `u64` | The size of the new file, in bytes |

An entry taken from an older version of a page may hold an older value, and an entry that the lost page had deleted may come back. So a report that is not whole means the new file may differ from the commit in ways the counts cannot show.

## Methods

### is_whole

```rust
pub fn is_whole(&self) -> bool
```

Whether the new file holds exactly the commit salvage started from: there was one (`commit_id` is `Some`), every page of it was read, and no object was dropped.
