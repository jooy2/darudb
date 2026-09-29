---
title: Tools
order: 5
---

# Tools

DaruDB ships with tools for a file that has to be checked, copied, made smaller or rescued. The check, backup and compaction work while other handles and processes keep reading and writing the file; salvage is for a file nothing can use, and needs it alone.

## Check a file

The integrity check reads everything the published commit reaches and verifies it: every page against the check its parent recorded, the order of every key, every tree's count, that every page of the file is used, free or retained exactly once, and, in a file with a schema, every object against its indexes. It reports every problem it finds rather than stopping at the first, and it throws only when it cannot begin, as when the database is closed.

```rust
use darudb::Database;

fn check(db: &Database) -> Result<(), darudb::Error> {
    let report = db.check()?;

    if !report.is_ok() {
        for problem in &report.problems {
            eprintln!("{problem}");
        }
    }

    Ok(())
}
```

```ts
const report = db.check(); // or `await db.checkAsync()`

if (!report.ok) {
  for (const { page, tree, message } of report.problems) {
    console.error(page, tree, message);
  }
}
```

- A problem names the page it is in, where it is in one, and the tree or collection it was found in.
- The check reads the whole file, so it takes about as long as reading every object. It keeps one bit for each page in memory, and nothing that grows with the number of objects.
- A page that cannot be read hides the pages below it. The check counts those in one problem rather than reporting each as leaked.

## Back up a file

A backup writes a copy of the published commit to a new file, while other handles and processes keep reading and writing. The copy holds no free space, has the page size of the file, and opens with the same key or password when the file is encrypted.

```rust
use darudb::Database;

fn back_up(db: &Database) -> Result<(), darudb::Error> {
    let report = db.backup("backups/app.darudb")?;

    println!("{} entries of commit {}", report.entries, report.commit_id);
    Ok(())
}
```

```ts
const report = await db.backupAsync('backups/app.darudb'); // or `db.backup(path)`
```

- The copy is written under a temporary name beside the path and takes the path only once it is whole and durable. A backup never replaces a file: when the path is taken, it fails with `INVALID_ARGUMENT`.
- The backup holds the commit it copies for as long as it runs, as a read transaction does, so the file may grow meanwhile if others write.

## Compact a file

A file keeps the pages it once needed: deleting objects frees pages inside it, which later writes reuse, but the file does not shrink by itself. Compaction moves the pages at the end of the file into free pages nearer its start and gives the end back to the file system. It works in place, while other handles and processes keep reading and writing.

```rust
use darudb::Database;

fn compact(db: &Database) -> Result<(), darudb::Error> {
    let report = db.compact()?;

    println!("{} bytes, then {}", report.bytes_before, report.bytes_after);
    Ok(())
}
```

```ts
const report = await db.compactAsync(); // or `db.compact()`
```

- Compaction is made of ordinary write transactions, so it waits for the writer lock like any write, and a crash in the middle leaves the file at one of its commits.
- A page that a read transaction can still reach cannot move until the transaction ends, so the file shrinks less next to long readers. The next compaction takes the rest.
- To get a compact copy without touching the file, use a backup instead.

## Salvage a damaged file

When a file does not open, or the check finds damage, salvage rescues what it can into a new file. It reads the old file page by page instead of opening it, starts from the newest commit the file records, and where that commit's pages cannot be read, takes the same keys from older versions of those pages that are still in the file. It then builds every index again from the objects, so the new file passes the check.

```rust
use darudb::OpenOptions;

fn rescue() -> Result<(), darudb::Error> {
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

```ts
import { Database } from 'darudb';

const report = await Database.salvageAsync('app.darudb', 'rescued.darudb'); // or `Database.salvage`

if (!report.whole) {
  console.warn(report.entriesRecovered, report.valuesLost, report.objectsDropped);
}
```

- The report is whole (`is_whole()` in Rust, `whole` in TypeScript) when the new file holds exactly the newest commit. Otherwise older versions filled what could not be read: an entry may have an older value, and one that a lost page had deleted may come back.
- An object whose record cannot be read, or whose value of a unique index another object has taken, is left out and counted.
- Salvage needs the file to itself. A file open in any process fails with `BUSY`, and so does opening the file while salvage runs.
- The new file has the page size of the old one. An encrypted file is salvaged with its key or password in the options, which then opens the new file too. Like a backup, salvage never replaces a file already at the path.
- It reads the whole file about twice, and keeps the first and last key of every page of entries in memory, so a large file needs memory in proportion.
