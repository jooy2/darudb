//! The phase 1 crash suite: random transactions on a simulated disk, cut at
//! random points by a power failure or by the process dying, then opened
//! again and compared with the history of commits.
//!
//! What must hold after every cut, as `design/commits-and-recovery.md` states
//! it: the file opens; its contents are exactly the state after a commit no
//! older than the last one that returned and no newer than the one in flight;
//! and every page of the file is accounted for exactly once.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::btree::{Load, Node};
use crate::format::{
    CATALOG_TREE, FREE_TREE, Pointer, RETAINED_TREE, StoredValue, TreeDescriptor, decode_free_key,
    decode_free_value, decode_retained_key, decode_runs,
};
use crate::storage::sim::SimDisk;
use crate::testing::Rng;
use crate::{Database, Error, OpenOptions};

/// Every tree's entries: the whole observable state of a database.
type State = BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>;

const TREES: [&str; 3] = ["apples", "books", "cars"];

/// Reads everything through the public API.
fn contents(db: &Database) -> State {
    let read = db.begin_read().unwrap();
    let mut state = State::new();

    for name in read.tree_names().unwrap() {
        let entries = read
            .iter(&name)
            .unwrap()
            .collect::<Result<BTreeMap<_, _>, _>>()
            .unwrap();

        assert_eq!(
            read.len(&name).unwrap(),
            entries.len() as u64,
            "tree {name}"
        );
        state.insert(name, entries);
    }

    state
}

/// Checks that every page of the published commit is reachable exactly once:
/// live in a tree, free, or retained. Returns what is wrong, if anything.
pub(crate) fn check_integrity(db: &Database) -> Result<(), String> {
    let shared = db.shared();
    let record = shared
        .header()
        .published()
        .map_err(|error| error.to_string())?;
    let loader = &shared.loader;
    let mut owners: HashMap<u64, String> = HashMap::new();
    let mut claim = |page: u64, what: &str| -> Result<(), String> {
        if page == 0 || page >= record.page_count {
            return Err(format!("{what} uses page {page}, outside the file"));
        }

        match owners.insert(page, what.to_owned()) {
            None => Ok(()),
            Some(other) => Err(format!("page {page} is used by both {other} and {what}")),
        }
    };
    let mut trees = vec![(record.catalog, CATALOG_TREE, "the catalog".to_owned())];
    let mut free_runs = Vec::new();
    let mut retained_runs = Vec::new();

    trees.push((record.free, FREE_TREE, "the free tree".to_owned()));
    trees.push((
        record.retained,
        RETAINED_TREE,
        "the retained tree".to_owned(),
    ));

    while let Some((root, tree, name)) = trees.pop() {
        let mut entries = 0u64;
        let mut pending: Vec<(Pointer, Option<u8>)> = vec![(root, None)];

        if root.is_null() {
            continue;
        }

        while let Some((pointer, level)) = pending.pop() {
            claim(pointer.page, &name)?;

            let loaded = loader
                .load(&pointer, tree, level)
                .map_err(|error| error.to_string())?;

            match &loaded.node {
                Node::Branch(branch) => {
                    for child in &branch.children {
                        if let crate::btree::Child::Clean(child) = child {
                            pending.push((*child, Some(branch.level - 1)));
                        }
                    }
                }
                Node::Leaf(leaf) => {
                    for entry in leaf {
                        entries += 1;

                        let value = match &entry.value {
                            StoredValue::Inline(value) => value.clone(),
                            StoredValue::Overflow(reference) => {
                                for index in 0..u64::from(reference.pages) {
                                    claim(reference.first + index, &format!("a value of {name}"))?;
                                }

                                loader
                                    .read_overflow(reference, tree)
                                    .map_err(|error| error.to_string())?
                            }
                        };

                        match tree {
                            CATALOG_TREE => {
                                let descriptor = TreeDescriptor::decode(&value)?;
                                let tree_name = String::from_utf8_lossy(&entry.key).into_owned();

                                trees.push((descriptor.root, descriptor.id, tree_name.clone()));

                                if descriptor.root.is_null() != (descriptor.entries == 0) {
                                    return Err(format!(
                                        "{tree_name}'s count disagrees with its root"
                                    ));
                                }
                            }
                            FREE_TREE => {
                                free_runs.push((
                                    decode_free_key(&entry.key)?,
                                    decode_free_value(&value)?,
                                ));
                            }
                            RETAINED_TREE => {
                                let (group, _) = decode_retained_key(&entry.key)?;

                                if group > record.txn {
                                    return Err(format!("a retained group from commit {group}"));
                                }

                                for (start, len) in decode_runs(&value)? {
                                    retained_runs.push((start, u64::from(len)));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if tree >= crate::format::FIRST_USER_TREE {
            let read = db.begin_read().map_err(|error| error.to_string())?;

            if read.len(&name).map_err(|error| error.to_string())? != entries {
                return Err(format!(
                    "{name} holds {entries} entries, not what its descriptor says"
                ));
            }
        }
    }

    for (start, len) in free_runs {
        for page in start..start + len {
            claim(page, "the free pages")?;
        }
    }

    for (start, len) in retained_runs {
        for page in start..start + len {
            claim(page, "the retained pages")?;
        }
    }

    for page in 1..record.page_count {
        if !owners.contains_key(&page) {
            return Err(format!(
                "page {page} is neither used, free nor retained: it leaked"
            ));
        }
    }

    Ok(())
}

/// One random write transaction: its operations applied to a copy of the
/// model, and whether it ends in a commit.
fn random_transaction(
    rng: &mut Rng,
    db: &Database,
    model: &State,
    page_size: usize,
) -> (crate::Result<()>, State, bool) {
    let mut next = model.clone();
    let mut txn = match db.begin_write() {
        Ok(txn) => txn,
        Err(error) => return (Err(error), next, false),
    };

    for _ in 0..1 + rng.index(24) {
        let tree = TREES[rng.index(TREES.len())];
        let key: Vec<u8> = (0..1 + rng.index(6))
            .map(|_| b'a' + u8::try_from(rng.below(5)).unwrap())
            .collect();
        let result = match rng.below(20) {
            0 => txn.delete_tree(tree).map(|existed| {
                assert_eq!(existed, next.remove(tree).is_some());
            }),
            1..=6 => txn.remove(tree, &key).map(|removed| {
                let expected = next.get_mut(tree).and_then(|entries| entries.remove(&key));

                assert_eq!(removed, expected.is_some());
            }),
            _ => {
                let len = match rng.below(12) {
                    0 => rng.index(3 * page_size),
                    1 => 0,
                    _ => rng.index(120),
                };
                let value = rng.bytes(len);

                txn.insert(tree, &key, &value).map(|()| {
                    next.entry(tree.to_owned()).or_default().insert(key, value);
                })
            }
        };

        if let Err(error) = result {
            return (Err(error), model.clone(), false);
        }
    }

    if rng.below(8) == 0 {
        txn.abort();

        return (Ok(()), model.clone(), false);
    }

    (txn.commit(), next, true)
}

/// Runs random transactions, cutting power or killing the process every few,
/// and checks what opening the file afterwards finds.
fn run(seed: u64, page_size: u32, steps: usize) {
    let mut rng = Rng::new(seed);
    let mut disk = Arc::new(SimDisk::default());
    let mut db = Database::create_io(disk.clone(), page_size).unwrap();
    let mut model = State::new();
    let page_size = page_size as usize;

    for step in 0..steps {
        let crash = rng.below(6) == 0;

        if crash {
            // Let the next transaction get a random way through its writes.
            disk.stop_after(rng.index(60));
        }

        let (result, attempted, committing) = random_transaction(&mut rng, &db, &model, page_size);

        if !crash {
            result.unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));

            if committing {
                model = attempted;
            }

            if step % 5 == 0 {
                check_integrity(&db)
                    .unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));
            }

            continue;
        }

        // The transaction either finished before the budget ran out, or
        // stopped somewhere inside. Either way, the process is gone now.
        let finished = result.is_ok() && committing;
        let power_cut = rng.below(2) == 0;
        let image = if power_cut {
            disk.power_cut(&mut rng)
        } else {
            disk.current()
        };

        drop(db);
        disk = Arc::new(SimDisk::from_image(image));
        db = Database::open_io(disk.clone(), &OpenOptions::new())
            .unwrap_or_else(|error| panic!("seed {seed} step {step}: reopening failed: {error}"));

        let found = contents(&db);

        check_integrity(&db).unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));

        if finished {
            // A commit that returned is durable.
            assert!(
                found == attempted,
                "seed {seed} step {step}: a returned commit was lost ({})",
                difference(&attempted, &found)
            );
        } else if committing && found == attempted {
            // The commit in flight made it: it was not reported, and it may.
        } else {
            assert!(
                found == model,
                "seed {seed} step {step}: not the last commit ({})",
                difference(&model, &found)
            );
        }

        model = found;
    }
}

/// Which trees and keys differ between two states, for a failure message.
fn difference(expected: &State, found: &State) -> String {
    let mut lines = Vec::new();

    for name in expected.keys().chain(found.keys()) {
        let (left, right) = (expected.get(name), found.get(name));

        if left == right
            || lines
                .iter()
                .any(|line: &String| line.starts_with(name.as_str()))
        {
            continue;
        }

        let keys = |state: Option<&BTreeMap<Vec<u8>, Vec<u8>>>| state.map_or(0, BTreeMap::len);
        let differing = left
            .into_iter()
            .flatten()
            .filter(|(key, value)| right.and_then(|entries| entries.get(*key)) != Some(*value))
            .count();

        lines.push(format!(
            "{name}: {} entries expected, {} found, {differing} missing or different",
            keys(left),
            keys(right)
        ));
    }

    lines.join("; ")
}

/// Runs the suite with `DARUDB_CRASH_SEEDS` seeds, 150 by default: enough to
/// be quick, and a longer run is one environment variable away.
#[test]
fn power_cuts_and_process_kills_never_lose_a_returned_commit() {
    let seeds = std::env::var("DARUDB_CRASH_SEEDS")
        .ok()
        .and_then(|seeds| seeds.parse().ok())
        .unwrap_or(150);

    for seed in 0..seeds {
        run(seed, if seed % 5 == 0 { 16384 } else { 4096 }, 60);
    }
}

#[test]
fn a_failed_barrier_makes_the_database_unusable_until_reopened() {
    let disk = Arc::new(SimDisk::default());
    let db = Database::create_io(disk.clone(), 4096).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"before", b"1").unwrap();
    txn.commit().unwrap();

    disk.fail_syncs();

    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"after", b"2").unwrap();

    let error = txn.commit().unwrap_err();

    assert_eq!(error.code(), "SYNC_FAILED");
    assert_eq!(db.begin_read().unwrap_err().code(), "SYNC_FAILED");
    assert_eq!(db.begin_write().unwrap_err().code(), "SYNC_FAILED");

    // The outcome is unknown: opening the file again finds it either way.
    let reopened = Database::open_io(
        Arc::new(SimDisk::from_image(disk.current())),
        &OpenOptions::new(),
    )
    .unwrap();
    let state = contents(&reopened);

    assert_eq!(state["t"].get(b"before".as_slice()), Some(&b"1".to_vec()));
    check_integrity(&reopened).unwrap();
}

#[test]
fn recovery_erases_a_newer_record_the_file_is_too_short_for() {
    let disk = Arc::new(SimDisk::default());
    let db = Database::create_io(disk.clone(), 4096).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"key", b"value").unwrap();
    txn.commit().unwrap();

    let header = db.shared().header();
    let published = header.published().unwrap();
    let slot = (header.selector.slot + 1) % crate::format::SLOT_COUNT;

    drop(db);

    // What a power cut can leave: a whole record of a later commit, whose
    // writes that grew the file were lost.
    let mut image = disk.current();
    let pages = image.len() as u64 / 4096;
    let stale = crate::format::CommitRecord {
        txn: published.txn + 1,
        page_count: pages + 3,
        ..published
    };
    let at = crate::format::slot_offset(slot);

    image[at..at + crate::format::RECORD_LEN].copy_from_slice(&stale.encode(slot));

    let disk = Arc::new(SimDisk::from_image(image));
    let db = Database::open_io(disk.clone(), &OpenOptions::new()).unwrap();

    assert_eq!(db.shared().header().records[slot], None);
    assert_eq!(
        crate::format::CommitRecord::decode(slot, &disk.current()[at..]).ok(),
        Some(None),
        "the record is erased, not just skipped, so a longer file cannot revive it"
    );
    assert_eq!(contents(&db)["t"].len(), 1);
}

#[test]
fn a_reader_keeps_its_snapshot_while_the_file_changes_under_it() {
    let db = Database::create_io(Arc::new(SimDisk::default()), 4096).unwrap();
    let mut txn = db.begin_write().unwrap();

    for index in 0..500u32 {
        txn.insert("t", &index.to_be_bytes(), &[1; 100]).unwrap();
    }

    txn.commit().unwrap();

    let snapshot = db.begin_read().unwrap();
    let before = snapshot.iter("t").unwrap().count();

    // Rewrite everything many times: the reader's pages must not be reused.
    for round in 0..20u8 {
        let mut txn = db.begin_write().unwrap();

        for index in 0..500u32 {
            txn.insert("t", &index.to_be_bytes(), &[round; 100])
                .unwrap();
        }

        txn.commit().unwrap();
        check_integrity(&db).unwrap();
    }

    let values: Vec<_> = snapshot
        .iter("t")
        .unwrap()
        .map(|entry| entry.unwrap().1)
        .collect();

    assert_eq!(values.len(), before);
    assert!(values.iter().all(|value| value == &vec![1; 100]));
    assert_eq!(
        db.begin_read()
            .unwrap()
            .get("t", &0u32.to_be_bytes())
            .unwrap(),
        Some(vec![19; 100])
    );
}

#[test]
fn rewriting_the_same_data_reuses_pages_once_no_reader_holds_them() {
    let disk = Arc::new(SimDisk::default());
    let db = Database::create_io(disk.clone(), 4096).unwrap();
    let write = |value: u8| {
        let mut txn = db.begin_write().unwrap();

        for index in 0..300u32 {
            txn.insert("t", &index.to_be_bytes(), &[value; 100])
                .unwrap();
        }

        txn.commit().unwrap();
    };

    write(0);
    write(1);

    let settled = disk.current().len();

    for value in 2..40 {
        write(value);
    }

    assert!(
        disk.current().len() <= settled + 4 * 4096,
        "the file grew from {settled} to {} bytes",
        disk.current().len()
    );
    check_integrity(&db).unwrap();
}

#[test]
fn a_damaged_page_is_reported_and_never_panics() {
    let disk = Arc::new(SimDisk::default());
    let db = Database::create_io(disk.clone(), 4096).unwrap();
    let mut txn = db.begin_write().unwrap();

    for index in 0..2000u32 {
        txn.insert("t", &index.to_be_bytes(), &[3; 60]).unwrap();
    }

    txn.commit().unwrap();
    drop(db);

    let mut rng = Rng::new(5);
    let image = disk.current();

    for _ in 0..40 {
        let mut damaged = image.clone();
        let page = 1 + rng.index(damaged.len() / 4096 - 1);
        let at = page * 4096 + rng.index(4096);

        damaged[at] ^= 1 << rng.below(8);

        // Opening may or may not touch the page; reading everything must
        // either succeed or say the file is damaged.
        match Database::open_io(Arc::new(SimDisk::from_image(damaged)), &OpenOptions::new()) {
            Ok(db) => {
                let read = db.begin_read().unwrap();
                let walked: Result<Vec<_>, Error> = match read.iter("t") {
                    Ok(range) => range.collect(),
                    Err(error) => Err(error),
                };

                if let Err(error) = walked {
                    assert_eq!(error.code(), "CORRUPTED", "{error}");
                }
            }
            Err(error) => assert_eq!(error.code(), "CORRUPTED", "{error}"),
        }
    }
}
