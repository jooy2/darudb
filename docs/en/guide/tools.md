---
title: Tools
order: 5
---

# Tools

DaruDB ships with tools for a file that has to be checked or rescued. Each one works while other handles and processes keep reading and writing the file.

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
