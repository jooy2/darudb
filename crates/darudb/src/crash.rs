//! The crash suite: random transactions on a simulated disk, cut at random
//! points by a power failure or by the process dying, then opened again and
//! compared with the history of commits. Half the runs use an encrypted file,
//! so every rule below holds with encryption on and off.
//!
//! What must hold after every cut, as `design/commits-and-recovery.md` states
//! it: the file opens; its contents are exactly the state after one commit;
//! and every page of the file is accounted for exactly once. After a process
//! dies, that commit is the last one that returned or the one in flight,
//! deferred commits included. After a power cut, it may also be any deferred
//! commit since the last barrier, or the durable commit before them.
//!
//! The object runs do the same with collections of objects, and check besides
//! that every index holds exactly what its objects give it, and that a
//! migration cut short leaves the file at the old schema or the new one.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

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

            match &loaded.to_node().map_err(|error| error.to_string())? {
                Node::Branch(branch) => {
                    for child in &branch.children {
                        if let crate::btree::Child::Clean(child) = child {
                            pending.push((*child, Some(branch.level - 1)));
                        }
                    }
                }
                Node::Leaf(leaf) => {
                    for entry in &leaf.to_entries().map_err(str::to_owned)? {
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

            // `len_in`, since the object layer's trees are among them.
            if read.len_in(&name).map_err(|error| error.to_string())? != entries {
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
/// model, and whether it ends in a commit, deferred if `deferred` says so.
fn random_transaction(
    rng: &mut Rng,
    db: &Database,
    model: &State,
    page_size: usize,
    deferred: bool,
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

    let result = if deferred {
        txn.commit_deferred()
    } else {
        txn.commit()
    };

    (result, next, true)
}

/// What one step of a run does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Commit,
    CommitDeferred,
    Sync,
}

/// Runs random transactions, cutting power or killing the process every few,
/// and checks what opening the file afterwards finds.
///
/// Some seeds commit only with sync commits, the others mostly with deferred
/// ones, some of them under a window limit small enough to be reached often.
/// Odd seeds encrypt the file, with a key under each of the two page ciphers
/// or, now and then, with a password under the cipher this machine prefers.
/// The window's time limit is left out: it depends on the clock, and the run
/// has to replay the same way from its seed.
fn run(seed: u64, page_size: u32, steps: usize) {
    let mut rng = Rng::new(seed);
    let mut options = OpenOptions::new();

    options.max_unsynced_time(Duration::MAX);

    if seed % 3 == 1 {
        options.max_unsynced_pages(24);
    }

    let key = [u8::try_from(seed % 251).unwrap(); 32];

    match seed % 10 {
        3 => options.password("crash suite").password_hashing(8, 1, 1),
        1 | 5 => options
            .key(key)
            .pin_page_cipher(crate::format::Cipher::XChaCha20Poly1305),
        7 | 9 => options
            .key(key)
            .pin_page_cipher(crate::format::Cipher::Xaes256Gcm),
        _ => &mut options,
    };

    let defers = seed % 4 != 0;
    let mut disk = Arc::new(SimDisk::default());
    let mut db = Database::create_io(disk.clone(), page_size, &options).unwrap();
    let mut model = State::new();
    // The state after each commit since the last one known to be durable,
    // that one first: what a power cut may go back to.
    let mut history = vec![State::new()];
    let page_size = page_size as usize;

    for step in 0..steps {
        let crash = rng.below(6) == 0;
        let action = match rng.below(12) {
            0 if defers => Action::Sync,
            1..=8 if defers => Action::CommitDeferred,
            _ => Action::Commit,
        };

        if crash {
            // Let the next step get a random way through its writes.
            disk.stop_after(rng.index(60));
        }

        let (result, attempted, committing) = match action {
            Action::Sync => (db.sync(), model.clone(), false),
            _ => random_transaction(
                &mut rng,
                &db,
                &model,
                page_size,
                action == Action::CommitDeferred,
            ),
        };

        if !crash {
            result.unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));

            match action {
                Action::Sync => history = vec![model.clone()],
                Action::Commit if committing => history = vec![attempted.clone()],
                Action::CommitDeferred if committing => history.push(attempted.clone()),
                _ => {}
            }

            if committing {
                model = attempted;
            }

            if step % 5 == 0 {
                check_integrity(&db)
                    .unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));
            }

            continue;
        }

        // The step either finished before the budget ran out, or stopped
        // somewhere inside. Either way, the process is gone now.
        let finished = result.is_ok();
        let power_cut = rng.below(2) == 0;
        let image = if power_cut {
            disk.power_cut(&mut rng)
        } else {
            disk.current()
        };

        drop(db);
        disk = Arc::new(SimDisk::from_image(image));
        db = Database::open_io(disk.clone(), &options)
            .unwrap_or_else(|error| panic!("seed {seed} step {step}: reopening failed: {error}"));

        let found = contents(&db);

        check_integrity(&db).unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));

        // What the file may hold: the last state that returned, or the commit
        // in flight. A barrier that returned makes the last state the only
        // one; otherwise a power cut may go back into the history.
        let latest = if finished && committing {
            &attempted
        } else {
            &model
        };
        let barrier_returned =
            finished && (action == Action::Sync || (action == Action::Commit && committing));
        let allowed = found == *latest
            || (committing && !finished && found == attempted)
            || (power_cut && !barrier_returned && history.contains(&found));

        assert!(
            allowed,
            "seed {seed} step {step}: {action:?} {} with a {}, and the file holds none of the \
             states it may ({} against the last)",
            if finished {
                "returned"
            } else {
                "was cut short"
            },
            if power_cut {
                "power cut"
            } else {
                "process kill"
            },
            difference(latest, &found)
        );

        model = found.clone();
        history = vec![found];
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

/// Runs the suite with `DARUDB_CRASH_SEEDS` seeds, 400 by default: enough to
/// be quick, and a longer run is one environment variable away.
#[test]
fn power_cuts_and_process_kills_never_lose_a_returned_commit() {
    let seeds = std::env::var("DARUDB_CRASH_SEEDS")
        .ok()
        .and_then(|seeds| seeds.parse().ok())
        .unwrap_or(400);

    for seed in 0..seeds {
        run(seed, if seed % 5 == 0 { 16384 } else { 4096 }, 60);
    }
}

#[test]
fn a_failed_barrier_makes_the_database_unusable_until_reopened() {
    let disk = Arc::new(SimDisk::default());
    let db = Database::create_io(disk.clone(), 4096, &OpenOptions::new()).unwrap();
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
    let db = Database::create_io(disk.clone(), 4096, &OpenOptions::new()).unwrap();
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
fn a_power_cut_while_the_key_changes_leaves_the_old_key_or_the_new_one() {
    let mut old = OpenOptions::new();
    let mut new = OpenOptions::new();

    old.key([1; 32]);
    new.key([2; 32]);

    for budget in 0..40 {
        let disk = Arc::new(SimDisk::default());
        let db = Database::create_io(disk.clone(), 4096, &old).unwrap();
        let mut txn = db.begin_write().unwrap();

        txn.insert("t", b"key", b"value").unwrap();
        txn.commit().unwrap();
        disk.stop_after(budget);

        let changed = db.set_key([2; 32]).is_ok();
        let mut rng = Rng::new(budget as u64);

        for cut in 0..8 {
            let image = if cut == 0 {
                disk.current()
            } else {
                disk.power_cut(&mut rng)
            };
            let opened_old = Database::open_io(Arc::new(SimDisk::from_image(image.clone())), &old);
            let opened_new = Database::open_io(Arc::new(SimDisk::from_image(image)), &new);

            if changed && cut == 0 {
                assert_eq!(
                    opened_old.as_ref().err().map(Error::code),
                    Some("WRONG_KEY"),
                    "budget {budget}: the old key still opens the file"
                );
            }

            let db = match (opened_old, opened_new) {
                (Ok(db), _) | (_, Ok(db)) => db,
                (Err(old), Err(new)) => panic!("budget {budget}: neither key opens: {old}, {new}"),
            };

            assert_eq!(contents(&db)["t"].len(), 1, "budget {budget}");
            check_integrity(&db).unwrap();
        }
    }
}

#[test]
fn a_record_assembled_from_existing_pages_is_refused_in_an_encrypted_file() {
    let mut encrypted = OpenOptions::new();

    encrypted.key([6; 32]);

    for (options, expected) in [(OpenOptions::new(), b"old"), (encrypted, b"new")] {
        let disk = Arc::new(SimDisk::default());
        let db = Database::create_io(disk.clone(), 4096, &options).unwrap();

        for value in [b"old", b"new"] {
            let mut txn = db.begin_write().unwrap();

            txn.insert("t", b"key", value).unwrap();
            txn.commit().unwrap();
        }

        let header = db.shared().header();
        let newer = header.published().unwrap();
        let older = header
            .records
            .iter()
            .flatten()
            .find(|record| record.txn == newer.txn - 1)
            .copied()
            .unwrap();
        let slot = (0..crate::format::SLOT_COUNT)
            .find(|slot| header.records[*slot].is_none_or(|record| record.txn < older.txn))
            .unwrap();

        drop(db);

        // A newer commit that no one made: the older catalog, whose pages are
        // still there, with everything else of the newer commit. The record
        // check is a plain hash anyone can compute; the MAC is the newer one's.
        let forged = crate::format::CommitRecord {
            txn: newer.txn + 1,
            durable_txn: newer.txn,
            catalog: older.catalog,
            ..newer
        };
        let mut image = disk.current();
        let at = crate::format::slot_offset(slot);

        image[at..at + crate::format::RECORD_LEN].copy_from_slice(&forged.encode(slot));

        let db = Database::open_io(Arc::new(SimDisk::from_image(image)), &options).unwrap();

        assert_eq!(contents(&db)["t"][b"key".as_slice()], expected);

        // A plain file has no key to tell the two apart. It takes the forgery,
        // whose catalog and allocator trees disagree about which pages are in
        // use; that is the damage the record MAC keeps out of encrypted files.
        if db.is_encrypted() {
            check_integrity(&db).unwrap();
        } else {
            assert!(check_integrity(&db).is_err());
        }
    }
}

#[test]
fn a_reader_keeps_its_snapshot_while_the_file_changes_under_it() {
    let db = Database::create_io(Arc::new(SimDisk::default()), 4096, &OpenOptions::new()).unwrap();
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
    let db = Database::create_io(disk.clone(), 4096, &OpenOptions::new()).unwrap();
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
fn deferred_commits_reuse_the_pages_they_write_before_the_window_ends() {
    let disk = Arc::new(SimDisk::default());
    let mut options = OpenOptions::new();

    // A window that only `sync` ends.
    options
        .max_unsynced_pages(u64::MAX)
        .max_unsynced_time(Duration::MAX);

    let db = Database::create_io(disk.clone(), 4096, &options).unwrap();
    let mut txn = db.begin_write().unwrap();

    for index in 0..2000u32 {
        txn.insert("t", &index.to_be_bytes(), &[0; 100]).unwrap();
    }

    txn.commit().unwrap();

    let write = |round: u32| {
        let mut txn = db.begin_write().unwrap();

        // Ten keys, in ten different leaves.
        txn.insert("t", &(round % 10 * 200).to_be_bytes(), &[1; 100])
            .unwrap();
        txn.commit_deferred().unwrap();
    };

    for round in 0..20 {
        write(round);
    }

    // Every leaf they change has been copied once in the window by now: the
    // durable commit's copy stays until the window ends.
    let settled = disk.current().len();

    // Each of these writes a leaf, its branches and the catalog again.
    // Without reusing the pages of the commits before it inside the window,
    // the file grows by all of them.
    for round in 20..500 {
        write(round);
    }

    assert!(
        disk.current().len() <= settled + 16 * 4096,
        "the file grew from {settled} to {} bytes",
        disk.current().len()
    );
    check_integrity(&db).unwrap();
    db.sync().unwrap();
    check_integrity(&db).unwrap();
}

#[test]
fn a_window_counts_a_page_it_writes_again_once() {
    let disk = Arc::new(SimDisk::default());
    let mut options = OpenOptions::new();

    // Room for the pages a few commits write, where 500 of them write
    // thousands.
    options
        .max_unsynced_pages(128)
        .max_unsynced_time(Duration::MAX);

    let db = Database::create_io(disk.clone(), 4096, &options).unwrap();
    let mut txn = db.begin_write().unwrap();

    for index in 0..2000u32 {
        txn.insert("t", &index.to_be_bytes(), &[0; 100]).unwrap();
    }

    txn.commit().unwrap();

    let write = |round: u32| {
        let mut txn = db.begin_write().unwrap();

        // Ten keys, in ten different leaves.
        txn.insert("t", &(round % 10 * 200).to_be_bytes(), &[1; 100])
            .unwrap();
        txn.commit_deferred().unwrap();
    };

    for round in 0..20 {
        write(round);
    }

    let barriers = disk.syncs();

    for round in 20..500 {
        write(round);
    }

    assert_eq!(disk.syncs(), barriers, "the window reached its page limit");

    db.sync().unwrap();
    assert_eq!(disk.syncs(), barriers + 1);
    check_integrity(&db).unwrap();
}

#[test]
fn a_reader_in_the_window_keeps_its_snapshot_while_deferred_commits_reuse_pages() {
    let disk = Arc::new(SimDisk::default());
    let mut options = OpenOptions::new();

    // The smallest cache, so that the reader reads its pages from the file
    // rather than from copies the cache kept.
    options
        .max_unsynced_pages(u64::MAX)
        .max_unsynced_time(Duration::MAX)
        .cache_size(0);

    let db = Database::create_io(disk.clone(), 4096, &options).unwrap();
    let write = |value: u8, deferred: bool| {
        let mut txn = db.begin_write().unwrap();

        for index in 0..2000u32 {
            txn.insert("t", &index.to_be_bytes(), &[value; 100])
                .unwrap();
        }

        if deferred {
            txn.commit_deferred().unwrap();
        } else {
            txn.commit().unwrap();
        }
    };

    write(0, false);
    write(1, true);
    write(2, true);

    // A snapshot newer than the durable commit, whose pages were all written
    // in the window.
    let snapshot = db.begin_read().unwrap();

    for value in 3..30 {
        write(value, true);
        check_integrity(&db).unwrap();
    }

    let values: Vec<_> = snapshot
        .iter("t")
        .unwrap()
        .map(|entry| entry.unwrap().1)
        .collect();

    assert_eq!(values.len(), 2000);
    assert!(values.iter().all(|value| value == &vec![2; 100]));

    // Once the reader is gone, the pages it held go back into use.
    drop(snapshot);

    let grown = disk.current().len();

    for value in 30..60 {
        write(value, true);
    }

    assert!(
        disk.current().len() <= grown + 4 * 4096,
        "the file grew from {grown} to {} bytes",
        disk.current().len()
    );
    check_integrity(&db).unwrap();
}

#[test]
fn a_damaged_page_is_reported_and_never_panics() {
    let mut chacha = OpenOptions::new();
    let mut xaes = OpenOptions::new();

    chacha
        .key([9; 32])
        .pin_page_cipher(crate::format::Cipher::XChaCha20Poly1305);
    xaes.key([9; 32])
        .pin_page_cipher(crate::format::Cipher::Xaes256Gcm);

    for options in [OpenOptions::new(), chacha, xaes] {
        damage_pages(&options);
    }
}

fn damage_pages(options: &OpenOptions) {
    let disk = Arc::new(SimDisk::default());
    let db = Database::create_io(disk.clone(), 4096, options).unwrap();
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
        match Database::open_io(Arc::new(SimDisk::from_image(damaged)), options) {
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

/// The objects of the collection `items`, by id.
type Objects = BTreeMap<i64, crate::Object>;

/// The schema of the object runs at `version`: version 2 adds a unique index
/// that a migration builds, and a field its function fills.
fn object_schema(version: u64) -> crate::Schema {
    use crate::{Collection, Schema, Type};

    let items = Collection::new("items")
        .optional("name", Type::String)
        .with_default("rank", Type::Int, 0)
        .optional("tags", Type::list(Type::String))
        .optional("blob", Type::Bytes)
        .unique("name")
        .index("rank")
        .index("tags");

    match version {
        1 => Schema::new(1).collection(items),
        _ => Schema::new(2).collection(
            items
                .optional("double", Type::Int)
                .optional("label", Type::String)
                .unique("label"),
        ),
    }
}

fn object_options(version: u64, seed: u64) -> OpenOptions {
    let mut options = OpenOptions::new();

    options
        .max_unsynced_time(Duration::MAX)
        .schema(object_schema(version));

    if version == 2 {
        options.migration(crate::Migration::to(2).run(|migrating| {
            for key in migrating.previous_keys("items")? {
                let mut items = migrating.collection("items")?;
                let mut item = items.get(key)?.unwrap_or_default();
                let rank = item.get("rank").and_then(crate::Value::as_int).unwrap_or(0);

                item.set("double", rank * 2);
                item.set(
                    "label",
                    format!(
                        "item {}",
                        item.get("id").and_then(crate::Value::as_int).unwrap_or(0)
                    ),
                );
                items.put(item)?;
            }

            Ok(())
        }));
    }

    if seed % 2 == 1 {
        options.key([u8::try_from(seed % 251).unwrap(); 32]);
    }

    options
}

fn objects(db: &Database) -> Objects {
    let read = db.begin_read().unwrap();

    read.collection("items")
        .unwrap()
        .iter()
        .unwrap()
        .map(|object| {
            let object = object.unwrap();

            (
                object.get("id").and_then(crate::Value::as_int).unwrap(),
                object,
            )
        })
        .collect()
}

/// Checks that the indexes of the database's schema hold exactly what its
/// objects give them.
fn check_objects(db: &Database) -> Result<(), String> {
    let read = db.begin_read().map_err(|error| error.to_string())?;
    let schema = read.schema().cloned().ok_or("the database has no schema")?;

    crate::schema::objects::check_indexes(&read, &schema.schema)
}

fn random_item(rng: &mut Rng, page_size: usize) -> crate::Object {
    let mut item = crate::Object::new();

    if rng.below(4) == 0 {
        item.set("id", 1 + i64::try_from(rng.below(40)).unwrap());
    }

    if rng.below(2) == 0 {
        item.set("name", ["a", "b", "c", "d", "e", "f"][rng.index(6)]);
    }

    item.set("rank", i64::try_from(rng.below(5)).unwrap());

    if rng.below(2) == 0 {
        let tags: Vec<crate::Value> = (0..rng.below(3))
            .map(|_| ["x", "y", "z"][rng.index(3)].into())
            .collect();

        item.set("tags", tags);
    }

    if rng.below(6) == 0 {
        let len = if rng.below(3) == 0 {
            rng.index(3 * page_size)
        } else {
            rng.index(64)
        };

        item.set("blob", rng.bytes(len));
    }

    item
}

/// One random transaction of object writes, as [`random_transaction`] is of
/// tree writes. A write a unique index refuses leaves the model as it was.
fn random_object_transaction(
    rng: &mut Rng,
    db: &Database,
    model: &Objects,
    page_size: usize,
    deferred: bool,
) -> (crate::Result<()>, Objects, bool) {
    let mut next = model.clone();
    let mut txn = match db.begin_write() {
        Ok(txn) => txn,
        Err(error) => return (Err(error), next, false),
    };

    for _ in 0..1 + rng.index(12) {
        let mut items = match txn.collection("items") {
            Ok(items) => items,
            Err(error) => return (Err(error), model.clone(), false),
        };
        let result = match rng.below(4) {
            0 => {
                let id = 1 + i64::try_from(rng.below(40)).unwrap();

                items.delete(id).map(|existed| {
                    assert_eq!(existed, next.remove(&id).is_some());
                })
            }
            1 => items.put(random_item(rng, page_size)).map(drop),
            _ => items.insert(random_item(rng, page_size)).map(drop),
        };

        match result {
            Err(Error::DuplicateKey { .. }) => {}
            Err(error) => return (Err(error), model.clone(), false),
            Ok(()) => {
                // What the transaction holds now is the model, whatever the
                // write filled in.
                next = match items.iter() {
                    Ok(objects) => match objects.collect::<crate::Result<Vec<_>>>() {
                        Ok(objects) => objects
                            .into_iter()
                            .map(|object| {
                                (
                                    object.get("id").and_then(crate::Value::as_int).unwrap(),
                                    object,
                                )
                            })
                            .collect(),
                        Err(error) => return (Err(error), model.clone(), false),
                    },
                    Err(error) => return (Err(error), model.clone(), false),
                };
            }
        }
    }

    if rng.below(8) == 0 {
        txn.abort();

        return (Ok(()), model.clone(), false);
    }

    let result = if deferred {
        txn.commit_deferred()
    } else {
        txn.commit()
    };

    (result, next, true)
}

/// [`run`] for objects: random object transactions, cut by power failures
/// and process deaths, and after each cut, the objects are one of the states
/// the file may hold, every index agrees with them, and a new object gets a
/// number past every id there.
fn run_objects(seed: u64, steps: usize) {
    let mut rng = Rng::new(seed);
    let options = object_options(1, seed);
    let page_size = 4096;
    let mut disk = Arc::new(SimDisk::default());
    let mut db = Database::create_io(disk.clone(), 4096, &options).unwrap();
    let mut model = Objects::new();
    let mut history = vec![Objects::new()];

    for step in 0..steps {
        let crash = rng.below(5) == 0;
        let deferred = rng.below(2) == 0;

        if crash {
            disk.stop_after(rng.index(60));
        }

        let (result, attempted, committing) =
            random_object_transaction(&mut rng, &db, &model, page_size, deferred);

        if !crash {
            result.unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));

            if committing {
                if deferred {
                    history.push(attempted.clone());
                } else {
                    history = vec![attempted.clone()];
                }

                model = attempted;
            }

            continue;
        }

        let finished = result.is_ok();
        let power_cut = rng.below(2) == 0;
        let image = if power_cut {
            disk.power_cut(&mut rng)
        } else {
            disk.current()
        };

        drop(db);
        disk = Arc::new(SimDisk::from_image(image));
        db = Database::open_io(disk.clone(), &options)
            .unwrap_or_else(|error| panic!("seed {seed} step {step}: reopening failed: {error}"));

        let found = objects(&db);

        check_integrity(&db).unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));
        check_objects(&db).unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));

        let latest = if finished && committing {
            &attempted
        } else {
            &model
        };
        let barrier_returned = finished && committing && !deferred;
        let allowed = found == *latest
            || (committing && !finished && found == attempted)
            || (power_cut && !barrier_returned && history.contains(&found));

        assert!(
            allowed,
            "seed {seed} step {step}: the file holds {} objects, none of the states it may",
            found.len()
        );

        // The next number passes every id there.
        let mut txn = db.begin_write().unwrap();
        let id = txn
            .collection("items")
            .unwrap()
            .insert(crate::Object::new())
            .unwrap();

        assert!(
            found.keys().all(|existing| id.as_int() > Some(*existing)),
            "seed {seed} step {step}: a new object got {id:?}"
        );
        txn.abort();

        model = found.clone();
        history = vec![found];
    }
}

/// Runs the object runs with a quarter of `DARUDB_CRASH_SEEDS`, 100 by
/// default.
#[test]
fn objects_and_their_indexes_survive_power_cuts_and_process_kills() {
    let seeds = std::env::var("DARUDB_CRASH_SEEDS")
        .ok()
        .and_then(|seeds| seeds.parse::<u64>().ok())
        .map_or(100, |seeds| seeds.div_ceil(4));

    for seed in 0..seeds {
        run_objects(seed, 40);
    }
}

#[test]
fn a_migration_cut_short_leaves_the_old_schema_or_the_new_one() {
    let mut rng = Rng::new(17);

    // The pager writes neighbouring pages together, so the migration takes
    // only a few operations: every cut from before its first write to after
    // its barrier, several times, so that power cuts keep different writes.
    for run in 0..96 {
        let cut = run % 12;
        let seed = u64::try_from(run).unwrap();
        let disk = Arc::new(SimDisk::default());
        let db = Database::create_io(disk.clone(), 4096, &object_options(1, seed)).unwrap();
        let mut txn = db.begin_write().unwrap();

        for index in 0..300 {
            let mut item = crate::Object::new().with("rank", index % 7);

            if index % 3 == 0 {
                item.set("tags", vec![crate::Value::from("x")]);
            }

            txn.collection("items").unwrap().insert(item).unwrap();
        }

        txn.commit().unwrap();

        let before = objects(&db);

        drop(db);
        disk.stop_after(cut);

        let migrated = Database::open_io(disk.clone(), &object_options(2, seed));
        let finished = migrated.is_ok();

        drop(migrated);

        let power_cut = run % 3 != 0;
        let image = if power_cut {
            disk.power_cut(&mut rng)
        } else {
            disk.current()
        };
        let disk = Arc::new(SimDisk::from_image(image));

        // The file holds version 1 with every object as it was, unless the
        // migration committed, and a migration whose open returned did.
        match Database::open_io(disk.clone(), &object_options(1, seed)) {
            Ok(db) => {
                assert!(
                    !finished,
                    "cut {cut}: the migration returned and did not last"
                );
                assert_eq!(objects(&db), before, "cut {cut}");
                check_objects(&db).unwrap_or_else(|error| panic!("cut {cut}: {error}"));
            }
            Err(Error::SchemaTooNew { .. }) => {}
            Err(error) => panic!("cut {cut}: {error}"),
        }

        // Either way, opening with version 2 ends with the migrated objects.
        let db = Database::open_io(disk, &object_options(2, seed))
            .unwrap_or_else(|error| panic!("cut {cut}: {error}"));
        let after = objects(&db);

        check_integrity(&db).unwrap_or_else(|error| panic!("cut {cut}: {error}"));
        check_objects(&db).unwrap_or_else(|error| panic!("cut {cut}: {error}"));
        assert_eq!(after.len(), before.len(), "cut {cut}");

        for (id, item) in &after {
            let rank = item.get("rank").and_then(crate::Value::as_int).unwrap();

            assert_eq!(
                item.get("double"),
                Some(&crate::Value::Int(rank * 2)),
                "cut {cut}"
            );
            assert_eq!(
                item.get("label"),
                Some(&crate::Value::from(format!("item {id}")))
            );
        }
    }
}
