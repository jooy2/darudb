---
title: BackupReport
order: 6
---

# BackupReport

`BackupReport` is what a backup wrote: the commit it copied, how much of it, and the size of the new file.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupReport
```

[`Database::backup`](../../api/rust/database.md#backup) returns one once the new file is whole and durable. [Tools](../../guide/tools.md) explains how a backup works while others write. The struct is `#[non_exhaustive]`, so fields may be added in a release: read the fields by name, and use `..` when destructuring one.

```rust
use darudb::Database;

fn back_up(db: &Database) -> darudb::Result<()> {
    let report = db.backup("backups/app.darudb")?;

    println!("{} entries of commit {}, {} bytes", report.entries, report.commit_id, report.bytes);
    Ok(())
}
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `commit_id` | `u64` | The transaction id of the commit copied, the one published when the backup began |
| `trees` | `u64` | The trees copied, the engine's own included |
| `entries` | `u64` | The entries copied, in every tree |
| `bytes` | `u64` | The size of the new file, in bytes |

`trees` and `entries` count the engine's own trees as well as the application's: in a database with a schema, a collection is stored as one tree for its objects and one for each index, so these numbers are larger than the collections and objects it holds.
