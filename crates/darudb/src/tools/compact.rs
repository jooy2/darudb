//! Compaction: the file made smaller in place, while other handles and
//! processes go on using it (`design/tools.md`, "Compaction").
//!
//! A write transaction moves every page the file's tail holds into the
//! lowest free pages, which is where the allocator takes pages from anyway,
//! and the commits after it reclaim the tail and cut it off. It is nothing
//! but write transactions: the copy-on-write that keeps any commit safe
//! keeps these safe too, and a process that dies in the middle leaves the
//! file at one of their commits.

use crate::database::Database;
use crate::error::Result;

/// The rounds of moving and reclaiming a compaction makes at most. Each
/// round needs free pages below the tail for what it moves and for the
/// nodes on the way to it; one that finds too few moves less, and the next
/// round moves the rest.
const ROUNDS: usize = 4;

/// What a compaction did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompactReport {
    /// The size of the file before, in bytes.
    pub bytes_before: u64,
    /// The size of the file after, in bytes.
    pub bytes_after: u64,
    /// The pages moved out of the file's tail.
    pub pages_moved: u64,
}

pub(crate) fn compact(db: &Database) -> Result<CompactReport> {
    let pager = &db.shared().pager;
    let mut report = CompactReport {
        bytes_before: pager.file_len()?,
        ..CompactReport::default()
    };

    // Retained pages no snapshot needs become free, and a free tail goes.
    settle(db)?;

    for _ in 0..ROUNDS {
        let mut txn = db.begin_write()?;
        let (pages, free) = txn.space_summary();
        let used = pages - free;
        // Where the file would end with every page used moved below it,
        // with room for the nodes on the way to them.
        let threshold = used + (used / 64).max(4);

        if threshold >= pages {
            break;
        }

        let moved = txn.relocate_above(threshold)?;

        if moved == 0 {
            break;
        }

        txn.commit()?;
        report.pages_moved += moved;
        settle(db)?;

        let (after, _) = db.begin_write()?.space_summary();

        if after >= pages {
            break;
        }
    }

    report.bytes_after = pager.file_len()?;

    Ok(report)
}

/// Two empty sync commits: the first reclaims what no snapshot and no
/// recovery can reach any more, and cuts the free tail off the page count;
/// the second does the same for what the first gave up, and a sync commit
/// cuts the file to its page count.
fn settle(db: &Database) -> Result<()> {
    for _ in 0..2 {
        db.begin_write()?.commit()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{Collection, Object, OpenOptions, Schema, Type, Value};

    fn schema() -> Schema {
        Schema::new(1).collection(
            Collection::new("people")
                .field("name", Type::String)
                .optional("photo", Type::Bytes)
                .with_default("age", Type::Int, 0)
                .index("age"),
        )
    }

    /// A file grown by 4,000 people and emptied of most of them, whose
    /// survivors lie all over it.
    fn sparse(options: &OpenOptions, path: &std::path::Path) -> Database {
        let db = options.open(path).unwrap();
        let mut txn = db.begin_write().unwrap();

        {
            let mut people = txn.collection("people").unwrap();

            for n in 0..4_000 {
                let mut person = Object::new()
                    .with("name", format!("person {n}"))
                    .with("age", n % 60);

                // Now and then a photo too large for a leaf, on an object
                // that stays.
                if (n + 1) % 500 == 0 {
                    person.set("photo", vec![3u8; 9_000]);
                }

                people.insert(person).unwrap();
            }
        }

        txn.commit().unwrap();

        let mut txn = db.begin_write().unwrap();

        for id in 1..=4_000 {
            if id % 10 != 0 && id % 499 != 0 {
                txn.collection("people").unwrap().delete(id).unwrap();
            }
        }

        txn.commit().unwrap();
        db
    }

    fn people(db: &Database) -> Vec<Object> {
        db.begin_read()
            .unwrap()
            .collection("people")
            .unwrap()
            .iter()
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_sparse_file_shrinks_and_keeps_every_object() {
        let dir = tempfile::tempdir().unwrap();
        let mut keyed = OpenOptions::new();

        keyed.key([2; 32]);

        for (at, mut options) in [OpenOptions::new(), keyed].into_iter().enumerate() {
            options.schema(schema());

            let path = dir.path().join(format!("sparse-{at}.darudb"));
            let db = sparse(&options, &path);
            let before = people(&db);
            let report = db.compact().unwrap();

            assert!(report.pages_moved > 0, "{report:?}");
            assert!(report.bytes_after < report.bytes_before, "{report:?}");

            // What is left free is the room the moves needed on the way.
            let (pages, free) = db.begin_write().unwrap().space_summary();

            assert!(free * 8 < pages, "{free} of {pages} pages free: {report:?}");
            assert_eq!(report.bytes_after, fs::metadata(&path).unwrap().len());
            assert_eq!(people(&db), before);
            assert!(db.check().unwrap().is_ok(), "{:?}", db.check().unwrap());

            // Opened again, the file is whole, and holds the same.
            drop(db);

            let db = options.open(&path).unwrap();

            assert_eq!(people(&db), before);
            assert!(db.check().unwrap().is_ok());
            assert!(
                before
                    .iter()
                    .any(|person| person.get("photo").is_some_and(|photo| !photo.is_null())),
                "a photo survived"
            );
        }
    }

    /// A reader that holds a snapshot keeps the pages it can reach, so the
    /// file cannot shrink past them; the compaction still moves what it can,
    /// and the reader goes on reading its commit whole.
    #[test]
    fn a_reader_keeps_its_snapshot_through_a_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        let db = sparse(&options, &dir.path().join("held.darudb"));
        let before = people(&db);
        let read = db.begin_read().unwrap();

        db.compact().unwrap();

        let held: Vec<Object> = read
            .collection("people")
            .unwrap()
            .iter()
            .unwrap()
            .map(Result::unwrap)
            .collect();

        assert_eq!(held, before);
        drop(read);

        // With the reader gone, a second compaction takes the rest.
        let report = db.compact().unwrap();

        assert!(report.bytes_after <= report.bytes_before);
        assert_eq!(people(&db), before);
        assert!(db.check().unwrap().is_ok());
    }

    /// Random trees with keys up to the longest a page takes and values up
    /// to three pages long, mostly deleted again, compact to a file that
    /// holds the same, passes the integrity check, and opens again.
    #[test]
    fn random_files_compact_to_what_they_held() {
        use std::collections::BTreeMap;

        use crate::testing::Rng;

        let dir = tempfile::tempdir().unwrap();

        for seed in 0..6 {
            let mut rng = Rng::new(seed);
            let mut options = OpenOptions::new();

            if seed % 2 == 1 {
                options.key([u8::try_from(seed).unwrap(); 32]);
            }

            let path = dir.path().join(format!("random-{seed}.darudb"));
            let db = options.open(&path).unwrap();
            let mut model: BTreeMap<(String, Vec<u8>), Vec<u8>> = BTreeMap::new();

            for round in 0..6 {
                let mut txn = db.begin_write().unwrap();

                for _ in 0..400 {
                    let tree = ["a", "b", "c"][rng.index(3)].to_owned();
                    let key_len = if rng.below(20) == 0 {
                        900
                    } else {
                        1 + rng.index(16)
                    };
                    let key = rng.bytes(key_len);

                    if rng.below(3) == 0 && round > 0 {
                        txn.remove(&tree, &key).unwrap();
                        model.remove(&(tree, key));

                        continue;
                    }

                    let len = if rng.below(15) == 0 {
                        rng.index(3 * 4096)
                    } else {
                        rng.index(200)
                    };
                    let value = rng.bytes(len);

                    txn.insert(&tree, &key, &value).unwrap();
                    model.insert((tree, key), value);
                }

                txn.commit().unwrap();
            }

            // Most of it goes, so the survivors lie all over the file.
            let mut txn = db.begin_write().unwrap();
            let doomed: Vec<(String, Vec<u8>)> = model
                .keys()
                .filter(|_| rng.below(5) != 0)
                .cloned()
                .collect();

            for (tree, key) in doomed {
                txn.remove(&tree, &key).unwrap();
                model.remove(&(tree, key));
            }

            txn.commit().unwrap();

            let contents = |db: &Database| {
                let read = db.begin_read().unwrap();
                let mut all = BTreeMap::new();

                for tree in read.tree_names().unwrap() {
                    for entry in read.iter(&tree).unwrap() {
                        let (key, value) = entry.unwrap();

                        all.insert((tree.clone(), key), value);
                    }
                }

                all
            };
            let report = db.compact().unwrap();

            assert!(
                report.bytes_after < report.bytes_before,
                "seed {seed}: {report:?}"
            );
            assert_eq!(contents(&db), model, "seed {seed}");
            assert!(
                db.check().unwrap().is_ok(),
                "seed {seed}: {:?}",
                db.check().unwrap()
            );
            drop(db);

            let db = options.open(&path).unwrap();

            assert_eq!(contents(&db), model, "seed {seed}");
            assert!(db.check().unwrap().is_ok(), "seed {seed}");
        }
    }

    #[test]
    fn a_dense_file_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        let db = options.open(dir.path().join("dense.darudb")).unwrap();
        let mut txn = db.begin_write().unwrap();

        txn.collection("people")
            .unwrap()
            .insert(
                Object::new()
                    .with("name", "only")
                    .with("age", Value::Int(3)),
            )
            .unwrap();
        txn.commit().unwrap();

        let report = db.compact().unwrap();

        assert_eq!(report.pages_moved, 0);
        assert!(report.bytes_after <= report.bytes_before);
        assert!(db.check().unwrap().is_ok());
    }
}
