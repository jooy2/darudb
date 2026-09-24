//! The locks between real processes. Each test runs a helper, another process
//! of this test binary, because a process never conflicts with its own locks
//! on Unix-like systems.

use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::{LockError, Locks, last_unreached};
use crate::storage::DbFile;
use crate::testing::{HELPER_PATH, Helper, wait_to_be_told};
use crate::{Database, OpenOptions};

/// The helper: opens the database, says so, and keeps it open until killed.
#[test]
fn helper_holding_the_file_open() {
    let Ok(path) = env::var(HELPER_PATH) else {
        return;
    };
    let _db = Database::open(path).unwrap();

    println!("open");

    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn a_second_process_waits_for_the_file_and_gives_up_with_busy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.darudb");
    let helper = Helper::spawn("lock::tests::helper_holding_the_file_open", &path);

    helper.wait_for("open");

    let error = OpenOptions::new()
        .busy_timeout(Duration::from_millis(100))
        .open(&path)
        .unwrap_err();

    assert_eq!(error.code(), "BUSY");

    // Its locks die with it.
    helper.kill();

    let db = OpenOptions::new()
        .busy_timeout(Duration::from_millis(100))
        .open(&path)
        .unwrap();

    assert!(db.begin_read().unwrap().tree_names().unwrap().is_empty());
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
