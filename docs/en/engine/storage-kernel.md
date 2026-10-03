---
title: Storage kernel
order: 2
languages: [rust]
---

# Storage kernel

The storage kernel is the layer under collections: named trees of byte keys and byte values, read and written in transactions, which a Rust program can use directly.

## Trees of bytes

A database holds any number of trees. A tree has a name and holds entries, each a key and a value, and both are bytes that the kernel gives no meaning. Everything the engine stores is kept in trees, collections and indexes included, and the kernel is what gives them transactions, durability, encryption and sharing between processes.

- **Names** are text, from 1 byte to as long as a key may be. Names that begin with a NUL character belong to the engine, which keeps collections and indexes under them: every call refuses one with `INVALID_ARGUMENT`, and `tree_names` leaves them out.
- **Keys order as unsigned bytes**, and a key that is a prefix of another sorts first. The kernel knows no other order, so encode a key so that its bytes sort the way you want: an unsigned integer in big-endian, for one, sorts by its value.
- **A key** is at most ⌊(page size − 268) / 4⌋ bytes: 957 with the default page size of 4096, 4029 at 16384 and 16317 at 65536. The limit guarantees that four entries always fit in a node. An empty key is a key like any other.
- **A value** is less than 4 GiB long. A value too large to keep in the tree's pages, more than about a quarter of a page with its key, is stored in pages of its own and read back whole.
- **A tree** is created by the first insert into it, and a tree that does not exist reads as empty. Removing every entry leaves the tree empty and still listed; `delete_tree` removes it.

## Write transactions

Every change goes through a write transaction, and becomes visible together with the others when the transaction commits.

```rust
use darudb::Database;

fn main() -> Result<(), darudb::Error> {
    let db = Database::open("app.darudb")?;

    let mut txn = db.begin_write()?;
    txn.insert("users", b"alice", b"admin")?;
    txn.insert("users", b"bob", b"member")?;
    txn.commit()?;

    let read = db.begin_read()?;
    assert_eq!(read.get("users", b"alice")?, Some(b"admin".to_vec()));

    // Keys come back in byte order.
    for entry in read.range("users", b"a".as_slice()..b"c".as_slice())? {
        let (key, value) = entry?;
        println!("{} = {}", String::from_utf8_lossy(&key), String::from_utf8_lossy(&value));
    }

    Ok(())
}
```

- `insert` stores a value under a key, replacing any value already there. `remove` takes a key out and returns whether it was there, and `delete_tree` deletes a whole tree.
- The transaction's own reads, `get`, `range`, `range_backward`, `iter`, `len` and `tree_names`, see its changes.
- Dropping the transaction, or calling `abort`, throws its changes away, and nothing it did reaches the file's committed state.
- There is one write transaction at a time for each file, across every process. `begin_write` waits for one already running, in this process or another, for up to the busy timeout, 5 seconds unless `OpenOptions::busy_timeout` says otherwise, and then fails with `BUSY`. A thread that begins a second write transaction while it holds one waits for itself, and fails the same way.
- Once `insert`, `remove` or `delete_tree` has failed, as for a key over the limit, the transaction can only be aborted: `commit` fails with `INVALID_ARGUMENT`. A name that begins with NUL is the exception, refused before the transaction is touched. Collections behave differently: an object write they refuse leaves the transaction able to commit.

## Read transactions

A read transaction sees one commit, its snapshot, for as long as it lives, whatever this process or another commits after it began. It never waits for the writer, and the writer never waits for it.

- `get`, `range`, `range_backward`, `iter` and `len` read one tree, and `tree_names` lists the trees.
- `commit_id` is the transaction id of the commit it sees. Transaction ids only grow, so comparing two tells whether anything was committed in between.
- No page its snapshot reaches is reused until it is dropped, in any process. A read transaction kept open for a long time makes the file grow while others write, so drop it once it has read what it needs.

## Ranges

`range` takes any range of byte strings and returns the entries whose keys lie in it, in key order. `range_backward` returns the same entries from the last down, and `iter` returns every entry. Each item is a key and its value, or an error when a page the walk needs is damaged, where the walk ends. A range borrows its transaction.

A tree whose keys grow, such as numbers in big-endian, keeps its newest entries at the end, where `range_backward` starts:

```rust
use darudb::Database;

fn append(db: &Database, sequence: u64, line: &str) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;

    // Big-endian, so that the keys' byte order is their numeric order.
    txn.insert("log", &sequence.to_be_bytes(), line.as_bytes())?;
    txn.commit_deferred()
}

fn newest(db: &Database, count: usize) -> Result<Vec<String>, darudb::Error> {
    let read = db.begin_read()?;
    let mut lines = Vec::new();

    for entry in read.range_backward::<&[u8]>("log", ..)?.take(count) {
        let (_, value) = entry?;

        lines.push(String::from_utf8_lossy(&value).into_owned());
    }

    Ok(lines)
}
```

To read every key with a prefix, start the range at the prefix and stop at the first key that does not begin with it:

```rust
use darudb::ReadTransaction;

fn with_prefix(read: &ReadTransaction, prefix: &[u8]) -> Result<Vec<Vec<u8>>, darudb::Error> {
    let mut keys = Vec::new();

    for entry in read.range("users", prefix..)? {
        let (key, _) = entry?;

        if !key.starts_with(prefix) {
            break;
        }

        keys.push(key);
    }

    Ok(keys)
}
```

## Durability

`commit` returns once the changes are durable: they survive a crash of the process and a power cut. `commit_deferred` returns without waiting for the disk. Readers see its changes at once, and they become durable at the next barrier: the next `commit`, a call to `Database::sync`, closing the database, or the limits on how much may wait, one second and 16,384 pages by default (`OpenOptions::max_unsynced_time` and `OpenOptions::max_unsynced_pages`). A crash of the process loses no deferred commit. A power cut may undo the newest ones, from the newest back, and never leaves a gap. [Commits and recovery](./commits-and-recovery.md) explains both.

## Trees and collections in one file

A database opened with a schema still has the kernel. Its trees and its collections live in one file, and one write transaction can change both and commit them together:

```rust
use darudb::{Database, Object};

fn add_user(db: &Database, png: &[u8]) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;

    txn.collection("users")?
        .insert(Object::new().with("name", "Alice"))?;
    txn.insert("thumbnails", b"alice", png)?;
    txn.commit()
}
```

`tree_names` lists only the kernel's trees: the collections are kept in trees of the engine's own.

## When to use the kernel

Collections are the place to start. The kernel suits data that is:

- **Bytes already**, or encoded in a format of your own: files, cached responses, serialized structures.
- **Ordered your way**: keys built so that one range finds what you need, such as a timestamp in big-endian followed by an id.
- **Simple enough** that nothing collections add would be used.

Collections give what the kernel does not: typed fields checked against a schema, indexes kept in step with the objects, queries that choose an index, migrations from one schema version to the next, and an integrity check of every object against its indexes. Above all, only Rust reaches the kernel. The Node.js package reads and writes collections only, so data that another language has to read belongs in a collection. A program in another language leaves the kernel's trees as they are, and backup, compaction and salvage keep them.

## The calls

| Call | What it does |
| --- | --- |
| `Database::begin_write` | Starts the write transaction, waiting for one already running for up to the busy timeout. |
| `Database::begin_read` | Starts a read transaction on the last commit. |
| `Database::sync` | Makes every deferred commit durable, whichever process made it. |
| `insert`, `remove`, `delete_tree` | Change a tree, in a [write transaction](../api/rust/write-transaction.md). |
| `get`, `range`, `range_backward`, `iter`, `len`, `tree_names` | Read, in a [read transaction](../api/rust/read-transaction.md) or a write transaction. A range is a [`Range`](../types/rust/range.md). |
| `commit`, `commit_deferred`, `abort` | End a write transaction. |
| `commit_id` | The transaction id of the commit a read transaction sees. |

[`Database`](../api/rust/database.md) and [`OpenOptions`](../api/rust/open-options.md) have the options that apply to every tree: the busy timeout, the page size, the cache size and the limits on deferred commits.
