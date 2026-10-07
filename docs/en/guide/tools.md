---
title: Tools
order: 9
---

# Tools

DaruDB ships with tools for a file that has to be checked, copied, made smaller or rescued. The check, backup and compaction work while other handles and processes keep reading and writing the file; salvage is for a file nothing can use, and needs it alone.

## Check a file

The integrity check reads everything the published commit reaches and verifies it: every page against the check its parent recorded, the order of every key, every tree's count, that every page of the file is used, free or retained exactly once, and, in a file with a schema, every object against its indexes. It reports every problem it finds rather than stopping at the first, and it fails only when it cannot begin, as when the database is closed.

::: lang rust

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

:::

::: lang node

```ts
const report = db.check(); // or `await db.checkAsync()`

if (!report.ok) {
  for (const { page, tree, message } of report.problems) {
    console.error(page, tree, message);
  }
}
```

:::

::: lang dart

```dart
final report = db.check(); // or `await db.checkAsync()`

if (!report.ok) {
  for (final problem in report.problems) {
    print('${problem.page} ${problem.tree} ${problem.message}');
  }
}
```

:::

::: lang python

```python
report = db.check()  # or `await db.check_async()`

if not report.ok:
    for problem in report.problems:
        print(problem.page, problem.tree, problem.message)
```

:::

- A problem names the page it is in, where it is in one, and the tree or collection it was found in.
- The check reads the whole file, so it takes about as long as reading every object. It keeps one bit for each page in memory, and nothing that grows with the number of objects.
- A page that cannot be read hides the pages below it. The check counts those in one problem rather than reporting each as leaked.

## Back up a file

A backup writes a copy of the published commit to a new file, while other handles and processes keep reading and writing. The copy holds no free space, has the page size of the file, and opens with the same key or password when the file is encrypted.

::: lang rust

```rust
use darudb::Database;

fn back_up(db: &Database) -> Result<(), darudb::Error> {
    let report = db.backup("backups/app.darudb")?;

    println!("{} entries of commit {}", report.entries, report.commit_id);
    Ok(())
}
```

:::

::: lang node

```ts
const report = await db.backupAsync('backups/app.darudb'); // or `db.backup(path)`

console.log(`${report.entries} entries of commit ${report.commitId}`);
```

:::

::: lang dart

```dart
final report = await db.backupAsync('backups/app.darudb'); // or `db.backup(path)`

print('${report.entries} entries of commit ${report.commitId}');
```

:::

::: lang python

```python
report = db.backup("backups/app.darudb")  # or `await db.backup_async(path)`

print(f"{report.entries} entries of commit {report.commit_id}")
```

:::

- The copy is written under a temporary name beside the path and takes the path only once it is whole and durable. A backup never replaces a file: when the path is taken, it fails with `INVALID_ARGUMENT`.
- The backup holds the commit it copies for as long as it runs, as a read transaction does, so the file may grow meanwhile if others write.

### Back up under a new key

Changing the key or the password of an encrypted file wraps its data key again, and leaves the data key, which encrypts every page, as it was. A backup given a key or a password of its own writes its copy under a new random data key instead, so a data key that may have been exposed stays behind with the old file. Put the copy in the old file's place once every handle to the old file is closed. A plain database's copy is encrypted the same way, which is how a plain database becomes an encrypted one.

::: lang rust

```rust
use darudb::{BackupOptions, Database};

fn rekey(db: &Database) -> Result<(), darudb::Error> {
    db.backup_with("app.rekeyed.darudb", BackupOptions::new().password("a new password"))?;
    Ok(())
}
```

:::

::: lang node

```ts
await db.backupAsync('app.rekeyed.darudb', { password: 'a new password' }); // or `db.backup(path, options)`
```

:::

::: lang dart

```dart
await db.backupAsync('app.rekeyed.darudb', password: 'a new password'); // or `db.backup(path, ...)`
```

:::

::: lang python

```python
db.backup("app.rekeyed.darudb", password="a new password")  # or `await db.backup_async(...)`
```

:::

## Compact a file

A file keeps the pages it once needed: deleting objects frees pages inside it, which later writes reuse, but the file does not shrink by itself. Compaction moves the pages at the end of the file into free pages nearer its start and gives the end back to the file system. It works in place, while other handles and processes keep reading and writing.

::: lang rust

```rust
use darudb::Database;

fn compact(db: &Database) -> Result<(), darudb::Error> {
    let report = db.compact()?;

    println!("{} bytes, then {}", report.bytes_before, report.bytes_after);
    Ok(())
}
```

:::

::: lang node

```ts
const report = await db.compactAsync(); // or `db.compact()`

console.log(`${report.bytesBefore} bytes, then ${report.bytesAfter}`);
```

:::

::: lang dart

```dart
final report = await db.compactAsync(); // or `db.compact()`

print('${report.bytesBefore} bytes, then ${report.bytesAfter}');
```

:::

::: lang python

```python
report = db.compact()  # or `await db.compact_async()`

print(f"{report.bytes_before} bytes, then {report.bytes_after}")
```

:::

- Compaction is made of ordinary write transactions, so it waits for the writer lock like any write, and a crash in the middle leaves the file at one of its commits.
- A page that a read transaction can still reach cannot move until the transaction ends, so the file shrinks less next to long readers. The next compaction takes the rest.
- To get a compact copy without touching the file, use a backup instead.

## Salvage a damaged file

When a file does not open, or the check finds damage, salvage rescues what it can into a new file. It reads the old file page by page instead of opening it, starts from the newest commit the file records, and where that commit's pages cannot be read, takes the same keys from older versions of those pages that are still in the file. It then builds every index again from the objects, so the new file passes the check.

::: lang rust

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

An encrypted file is salvaged with its key or password set on the `OpenOptions`.

:::

::: lang node

```ts
import { Database } from 'darudb';

const report = await Database.salvageAsync('app.darudb', 'rescued.darudb'); // or `Database.salvage`

if (!report.whole) {
  console.warn(report.entriesRecovered, report.valuesLost, report.objectsDropped);
}
```

An encrypted file is salvaged with its `key` or `password` in the third argument's options.

:::

::: lang dart

```dart
final report = await Database.salvageAsync('app.darudb', 'rescued.darudb'); // or `Database.salvage`

if (!report.whole) {
  print('${report.entriesRecovered} ${report.valuesLost} ${report.objectsDropped}');
}
```

An encrypted file is salvaged with its `key` or `password` among the named options.

:::

::: lang python

```python
import darudb

report = darudb.Database.salvage("app.darudb", "rescued.darudb")  # or `Database.salvage_async`

if not report.whole:
    print(report.entries_recovered, report.values_lost, report.objects_dropped)
```

An encrypted file is salvaged with its `key` or `password` among the keyword options.

:::

- The report is whole (<LangCode rust="is_whole()" node="whole" dart="whole" python="whole" />) when the new file holds exactly the newest commit. Otherwise older versions filled what could not be read: an entry may have an older value, and one that a lost page had deleted may come back.
- An object whose record cannot be read, or whose value of a unique index another object has taken, is left out and counted.
- Salvage needs the file to itself. A file open in any process fails with `BUSY`, and so does opening the file while salvage runs.
- The new file has the page size of the old one, and an encrypted file's key or password opens the new file too. Like a backup, salvage never replaces a file already at the path.
- It reads the whole file about twice, and keeps the first and last key of every page of entries in memory, so a large file needs memory in proportion.
