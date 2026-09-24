//! The locks between real processes. Each test runs a helper, another process
//! of this test binary, because a process never conflicts with its own locks
//! on Unix-like systems.

use std::env;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::sys::{is_network_name, is_network_type};
use super::{LockError, Locks, last_unreached, on_network_file_system};
use crate::format::{SELECTOR_OFFSET, Selector};
use crate::storage::DbFile;
use crate::testing::{HELPER_PATH, Helper, wait_to_be_told};
use crate::{Database, OpenOptions};

/// The options of the database in the file `path`: encrypted when its name
/// says so.
fn options_for(path: &Path) -> OpenOptions {
    let mut options = OpenOptions::new();

    options.max_unsynced_time(Duration::from_secs(3600));

    if path.to_string_lossy().contains("encrypted") {
        options.key([0x3C; 32]);
    }

    options
}

/// The helper: opens the database and runs the commands the test sends it,
/// one a line, answering each on a line that starts with `answer`.
///
/// - `put <key> <value>` commits `value` under `key` in tree `t`.
/// - `snapshot` begins a read transaction and keeps it.
/// - `get <key>` reads through the kept read transaction, or a new one.
/// - `sum` counts the entries of tree `t` in the kept read transaction and
///   adds up the first byte of every value.
/// - `hold-writer` begins a write transaction and keeps it.
/// - `release` ends what it keeps.
/// - `fork <marker>` forks; see [`forked_child`].
#[test]
fn helper_running_commands() {
    let Ok(path) = env::var(HELPER_PATH) else {
        return;
    };
    // The only handle, so that a forked child that drops it drops the
    // instance.
    #[cfg_attr(not(unix), expect(unused_mut, reason = "only a forked child drops it"))]
    let mut handle = Some(options_for(path.as_ref()).open(&path).unwrap());
    let mut snapshot = None;
    let mut writer = None;

    println!("\nanswer open");

    for line in io::stdin().lines() {
        let line = line.unwrap();
        let db = handle.as_ref().unwrap();
        let words: Vec<&str> = line.split_whitespace().collect();
        let outcome = match words.as_slice() {
            ["put", key, value] => db.begin_write().and_then(|mut txn| {
                txn.insert("t", key.as_bytes(), value.as_bytes())?;
                txn.commit()?;

                Ok("done".to_owned())
            }),
            ["snapshot"] => db.begin_read().map(|txn| {
                snapshot = Some(txn);

                "done".to_owned()
            }),
            ["get", key] => {
                let fresh;
                let txn = match &snapshot {
                    Some(txn) => txn,
                    None => {
                        fresh = db.begin_read().unwrap();

                        &fresh
                    }
                };

                txn.get("t", key.as_bytes()).map(|value| {
                    value.map_or("none".to_owned(), |value| String::from_utf8(value).unwrap())
                })
            }
            ["sum"] => snapshot.as_ref().unwrap().iter("t").and_then(|entries| {
                let mut count = 0u64;
                let mut sum = 0u64;

                for entry in entries {
                    let (_, value) = entry?;

                    count += 1;
                    sum += u64::from(value[0]);
                }

                Ok(format!("{count} {sum}"))
            }),
            ["hold-writer"] => db.begin_write().map(|txn| {
                writer = Some(txn);

                "done".to_owned()
            }),
            ["release"] => {
                snapshot = None;
                writer = None;

                Ok("done".to_owned())
            }
            #[cfg(unix)]
            ["fork", marker] => match super::sys::fork().unwrap() {
                None => forked_child(path.as_ref(), &mut handle, Path::new(marker)),
                Some(child) => Ok(format!("child {}", super::sys::wait_for(child).unwrap())),
            },
            _ => panic!("an unknown command: {line}"),
        };

        match outcome {
            Ok(answer) => println!("answer {answer}"),
            Err(error) => println!("answer error {}", error.code()),
        }
    }
}

/// A child forked from the helper, which holds the file open, no read or
/// write transaction on it. Every handle the child inherited has to behave as
/// closed; the child opens the file itself, reads through its own handle,
/// and drops the inherited ones, which must not release its own locks. It
/// says which snapshot it reads, waits for the test to create `marker`, and
/// ends. Its exit code says which check failed.
#[cfg(unix)]
fn forked_child(path: &Path, inherited: &mut Option<Database>, marker: &Path) -> ! {
    let exit = super::sys::exit_now;
    let Some(db) = inherited.as_ref() else {
        exit(10);
    };

    if db.begin_read().err().map(|error| error.code()) != Some("CLOSED") {
        exit(11);
    }

    if db.begin_write().err().map(|error| error.code()) != Some("CLOSED") {
        exit(12);
    }

    let Ok(own) = options_for(path).open(path) else {
        exit(13);
    };
    let Ok(read) = own.begin_read() else {
        exit(14);
    };

    // Closing an inherited descriptor would release every lock the child
    // holds on the file, its own snapshot's included.
    *inherited = None;

    println!("answer child-reading {}", read.commit_id());

    for _ in 0..3000 {
        if marker.exists() {
            exit(0);
        }

        thread::sleep(Duration::from_millis(10));
    }

    exit(15)
}

fn helper(path: &Path) -> Helper {
    let helper = Helper::spawn("lock::tests::helper_running_commands", path);

    assert_eq!(helper.answer(), "open");

    helper
}

#[test]
fn processes_share_one_file_and_see_each_other_s_commits() {
    let dir = tempfile::tempdir().unwrap();

    for name in ["plain.darudb", "encrypted.darudb"] {
        let path = dir.path().join(name);
        let mut other = helper(&path);
        let db = options_for(&path).open(&path).unwrap();
        let mut txn = db.begin_write().unwrap();

        txn.insert("t", b"mine", b"1").unwrap();
        txn.commit().unwrap();

        assert_eq!(other.ask("get mine"), "1");
        assert_eq!(other.ask("put theirs 2"), "done");
        assert_eq!(
            db.begin_read().unwrap().get("t", b"theirs").unwrap(),
            Some(b"2".to_vec())
        );

        // Deferred commits from both sides, and a sync that makes them all
        // durable whoever made them.
        let mut txn = db.begin_write().unwrap();

        txn.insert("t", b"deferred", b"3").unwrap();
        txn.commit_deferred().unwrap();

        assert_eq!(other.ask("get deferred"), "3");

        db.sync().unwrap();

        assert!(!selector(&db).unsynced);
    }
}

/// The selector in the file, read through the database's own handle: this
/// process may not open a second one.
fn selector(db: &Database) -> Selector {
    let bytes = db.shared().pager.read_header(SELECTOR_OFFSET + 1).unwrap();

    Selector::decode(bytes[SELECTOR_OFFSET]).unwrap()
}

#[test]
fn a_process_that_opens_a_file_another_has_open_does_not_recover_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.darudb");
    let db = options_for(&path).open(&path).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"k", b"deferred").unwrap();
    txn.commit_deferred().unwrap();

    let before = selector(&db);

    assert!(before.unsynced);

    // Recovery would find the published commit unsynced, check it, and write
    // the selector with the bit clear.
    let mut other = helper(&path);

    assert_eq!(selector(&db), before);
    assert_eq!(other.ask("get k"), "deferred");
}

#[test]
fn a_snapshot_in_another_process_keeps_every_page_it_reaches() {
    let dir = tempfile::tempdir().unwrap();

    for name in ["plain.darudb", "encrypted.darudb"] {
        let path = dir.path().join(name);
        let db = options_for(&path).open(&path).unwrap();
        let write = |value: u8, deferred: bool| {
            let mut txn = db.begin_write().unwrap();

            for key in 0..300u32 {
                txn.insert("t", &key.to_be_bytes(), &[value; 100]).unwrap();
            }

            if deferred {
                txn.commit_deferred().unwrap();
            } else {
                txn.commit().unwrap();
            }
        };

        write(1, false);

        let mut other = helper(&path);

        assert_eq!(other.ask("snapshot"), "done");

        // Every page of the snapshot is copied again and again. Without its
        // lock, the writer would reuse them.
        for round in 2..30 {
            write(round, round % 3 == 0);
        }

        assert_eq!(other.ask("sum"), "300 300");
        assert_eq!(other.ask("release"), "done");

        // Released, the pages go back into use.
        write(30, false);
        write(31, false);

        let settled = db.shared().pager.file_len().unwrap();

        for round in 32..40 {
            write(round, false);
        }

        assert!(db.shared().pager.file_len().unwrap() <= settled + 8 * 4096);
        crate::crash::check_integrity(&db).unwrap();
    }
}

#[test]
fn a_writer_in_another_process_holds_off_this_one_and_no_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.darudb");
    let mut other = helper(&path);
    let db = OpenOptions::new()
        .busy_timeout(Duration::from_millis(100))
        .open(&path)
        .unwrap();

    assert_eq!(other.ask("put k 1"), "done");
    assert_eq!(other.ask("hold-writer"), "done");
    assert_eq!(db.begin_write().unwrap_err().code(), "BUSY");
    assert_eq!(
        db.begin_read().unwrap().get("t", b"k").unwrap(),
        Some(b"1".to_vec())
    );
    assert_eq!(other.ask("release"), "done");

    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"k", b"2").unwrap();
    txn.commit().unwrap();

    assert_eq!(other.ask("get k"), "2");

    // A writer that dies holding the lock lets it go.
    assert_eq!(other.ask("hold-writer"), "done");
    other.kill();

    db.begin_write().unwrap().abort();
}

/// Locks on `path` through a handle of this process's own. The file is an
/// empty one, not a database: these tests take the locks directly.
fn locks_on(path: &std::path::Path) -> Locks {
    Locks::on(Arc::new(DbFile::open(path).unwrap()))
}

fn soon() -> Option<Instant> {
    Some(Instant::now() + Duration::from_millis(50))
}

/// The helper: takes the writer lock and a snapshot, and holds them until
/// told to let the snapshot go, one registration at a time.
#[test]
fn helper_holding_the_writer_lock_and_a_snapshot() {
    let Ok(path) = env::var(HELPER_PATH) else {
        return;
    };
    let locks = locks_on(path.as_ref());

    locks.lock_writer(None).unwrap();
    locks.register(10, None).unwrap();
    locks.register(10, None).unwrap();
    println!("locked");

    wait_to_be_told();
    locks.unregister(10);
    println!("one-left");

    wait_to_be_told();
    locks.unregister(10);
    println!("none-left");

    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn another_process_sees_the_writer_lock_and_the_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locks");

    std::fs::write(&path, b"").unwrap();

    let mut helper = Helper::spawn(
        "lock::tests::helper_holding_the_writer_lock_and_a_snapshot",
        &path,
    );
    let locks = locks_on(&path);

    helper.wait_for("locked");

    assert!(matches!(locks.lock_writer(soon()), Err(LockError::Busy)));
    assert!(
        !locks.snapshot_below(10).unwrap(),
        "snapshot 10 is not below 10"
    );
    assert!(locks.snapshot_below(11).unwrap());
    assert!(locks.snapshot_below(1 << 40).unwrap());
    assert_eq!(locks.reclaimable(&[4, 9, 10, 11, 30]).unwrap(), Some(10));

    // The lock goes with the last registration, not the first.
    helper.tell();
    helper.wait_for("one-left");

    assert!(locks.snapshot_below(11).unwrap());

    helper.tell();
    helper.wait_for("none-left");

    assert!(!locks.snapshot_below(11).unwrap());
    assert_eq!(locks.reclaimable(&[4, 9, 10, 11, 30]).unwrap(), Some(30));

    // The writer lock dies with the process.
    helper.kill();
    locks.lock_writer(soon()).unwrap();
    locks.unlock_writer();
}

#[test]
fn this_process_s_own_snapshots_hold_back_what_they_reach() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locks");

    std::fs::write(&path, b"").unwrap();

    let locks = locks_on(&path);

    locks.register(7, None).unwrap();

    assert_eq!(locks.reclaimable(&[3, 7, 8, 12]).unwrap(), Some(7));

    locks.unregister(7);

    assert_eq!(locks.reclaimable(&[3, 7, 8, 12]).unwrap(), Some(12));
    assert_eq!(locks.reclaimable(&[]).unwrap(), None);
}

#[test]
fn the_search_asks_once_when_nothing_is_held_back_and_bisects_otherwise() {
    let groups = [2, 3, 5, 8, 13, 21, 34, 55];

    for oldest in 0..60 {
        let mut questions = 0;
        let found = last_unreached::<()>(&groups, |group| {
            questions += 1;

            Ok(oldest < group)
        })
        .unwrap();
        let expected = groups
            .iter()
            .copied()
            .filter(|group| *group <= oldest)
            .max();

        assert_eq!(found, expected, "oldest snapshot {oldest}");

        if oldest >= 55 {
            assert_eq!(questions, 1, "oldest snapshot {oldest}");
        } else {
            assert!(questions <= 4, "{questions} questions for {oldest}");
        }
    }
}

#[test]
fn network_file_systems_are_told_apart_from_local_ones() {
    // NFS, and CIFS as a signed 32-bit and a 64-bit field report it.
    assert!(is_network_type(0x6969_i64));
    assert!(is_network_type(-11_317_950_i32));
    assert!(is_network_type(0xFF53_4D42_u32));
    // ext4, and FUSE, which is left to the lock call.
    assert!(!is_network_type(0xEF53_i64));
    assert!(!is_network_type(0x6573_5546_i64));

    assert!(is_network_name(b"smbfs"));
    assert!(is_network_name(b"nfs"));
    assert!(!is_network_name(b"apfs"));
    assert!(!is_network_name(b"nfsx"));

    // A temporary directory is on a local disk, before a file exists in it
    // and after.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");

    assert!(!on_network_file_system(&path, None));

    std::fs::write(&path, b"").unwrap();

    assert!(!on_network_file_system(
        &path,
        Some(&DbFile::open(&path).unwrap())
    ));
}

#[cfg(unix)]
#[test]
fn a_forked_child_uses_its_own_handle_and_keeps_its_own_locks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.darudb");
    let marker = dir.path().join("marker");
    let mut other = helper(&path);

    assert_eq!(other.ask("put k 1"), "done");

    let answer = other.ask(&format!("fork {}", marker.display()));
    let snapshot: u64 = answer
        .strip_prefix("child-reading ")
        .unwrap_or_else(|| panic!("the child answered `{answer}`"))
        .parse()
        .unwrap();

    // Nobody but the child reads, so the lock on its snapshot is its own.
    let locks = locks_on(&path);

    assert!(locks.snapshot_below(snapshot + 1).unwrap());

    std::fs::write(&marker, b"").unwrap();

    assert_eq!(other.answer(), "child 0");
    assert!(!locks.snapshot_below(snapshot + 1).unwrap());
    assert_eq!(other.ask("get k"), "1", "the parent's handle still works");
}
