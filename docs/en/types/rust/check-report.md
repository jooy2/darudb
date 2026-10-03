---
title: CheckReport
order: 5
---

# CheckReport

`CheckReport` is what the integrity check found: the commit it checked, how much it read, and every problem.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckReport
```

[`Database::check`](../../api/rust/database.md#check) returns one. The check fails only when it cannot begin, so damage shows up here rather than as an error, and the file passed when [`is_ok`](#is-ok) is true. [Tools](../../guide/tools.md) explains what the check reads.

The struct is `#[non_exhaustive]`, so fields may be added in a release: read the fields by name, and use `..` when destructuring one.

```rust
use darudb::Database;

fn check(db: &Database) -> darudb::Result<()> {
    let report = db.check()?;

    if !report.is_ok() {
        for problem in &report.problems {
            eprintln!("{problem}");
        }
    }

    Ok(())
}
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `commit_id` | `u64` | The transaction id of the commit checked, the one published when the check began |
| `page_count` | `u64` | The pages that commit counts, the header page included |
| `pages_checked` | `u64` | The pages read and verified |
| `objects_checked` | `u64` | The objects read and checked against their indexes |
| `problems` | `Vec<Problem>` | Every problem found, in the order found; empty when the file is whole |

`commit_id` is the id that [`ReadTransaction::commit_id`](../../api/rust/read-transaction.md) gives for the same commit.

## Methods

### is_ok

```rust
pub fn is_ok(&self) -> bool
```

Whether the check found nothing wrong: `problems` is empty.

## Problem

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Problem
```

One thing the check found wrong. It is `#[non_exhaustive]` too.

| Field     | Type             | Description                                                      |
| --------- | ---------------- | ---------------------------------------------------------------- |
| `page`    | `Option<u64>`    | The page the problem is in, when it is in one page               |
| `tree`    | `Option<String>` | The tree or collection it was found in, when it was found in one |
| `message` | `String`         | What is wrong                                                    |

`tree` is meant for a person to read. It holds a tree's name or a collection's name, but for the engine's own trees it can hold a description such as `the free tree`, so a program should not look it up as a name.

`Problem` implements `Display`, which writes the page and the tree where they are known, followed by the message: `page 12 of "users": ...`.
