//! Reading and writing through transactions, through the public API only.

// The helpers below fail the test that called them by panicking, the way a
// test does; `clippy.toml` allows that inside tests but not in helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::env;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::TestDir;
use darudb::{Database, OpenOptions};

fn open(dir: &TestDir) -> Database {
    Database::open(dir.path("app.darudb")).unwrap()
}

fn entries(db: &Database, tree: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let read = db.begin_read().unwrap();

    read.iter(tree).unwrap().map(Result::unwrap).collect()
}

#[test]
fn a_commit_is_there_after_the_file_is_opened_again() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    txn.insert("users", b"alice", b"1").unwrap();
    txn.insert("users", b"bob", b"2").unwrap();
    txn.insert("orders", b"o-1", &vec![9; 20_000]).unwrap();
    txn.commit().unwrap();
    db.close().unwrap();

    let db = open(&dir);
    let read = db.begin_read().unwrap();

    assert_eq!(read.get("users", b"alice").unwrap(), Some(b"1".to_vec()));
    assert_eq!(read.get("users", b"carol").unwrap(), None);
    assert_eq!(read.get("orders", b"o-1").unwrap(), Some(vec![9; 20_000]));
    assert_eq!(read.len("users").unwrap(), 2);
    assert_eq!(read.tree_names().unwrap(), ["orders", "users"]);
}

#[test]
fn an_aborted_transaction_leaves_nothing_behind() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"k", b"v").unwrap();
    txn.abort();

    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"other", b"v").unwrap();
    drop(txn);

    let read = db.begin_read().unwrap();

    assert!(read.tree_names().unwrap().is_empty());
    assert_eq!(read.get("t", b"k").unwrap(), None);
}

#[test]
fn a_write_transaction_sees_its_own_changes() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"a", b"1").unwrap();
    txn.insert("t", b"b", b"2").unwrap();

    assert_eq!(txn.get("t", b"a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(txn.len("t").unwrap(), 2);
    assert!(txn.remove("t", b"a").unwrap());
    assert!(!txn.remove("t", b"a").unwrap(), "already gone");
    assert_eq!(
        txn.iter("t")
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>(),
        [(b"b".to_vec(), b"2".to_vec())]
    );
    assert_eq!(txn.tree_names().unwrap(), ["t"]);
}

#[test]
fn a_reader_sees_only_the_commit_it_started_from() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"k", b"old").unwrap();
    txn.commit().unwrap();

    let before = db.begin_read().unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"k", b"new").unwrap();
    txn.insert("t", b"added", b"x").unwrap();

    assert_eq!(
        before.get("t", b"k").unwrap(),
        Some(b"old".to_vec()),
        "uncommitted"
    );

    txn.commit().unwrap();

    let after = db.begin_read().unwrap();

    assert_eq!(
        before.get("t", b"k").unwrap(),
        Some(b"old".to_vec()),
        "committed later"
    );
    assert_eq!(before.len("t").unwrap(), 1);
    assert_eq!(after.get("t", b"k").unwrap(), Some(b"new".to_vec()));
    assert!(after.commit_id() > before.commit_id());
}

#[test]
fn ranges_take_their_bounds_from_ordinary_range_syntax() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    for key in [b"a", b"b", b"c", b"d"] {
        txn.insert("t", key, b"").unwrap();
    }

    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let keys = |range: darudb::Range<'_>| -> Vec<Vec<u8>> {
        range.map(|entry| entry.unwrap().0).collect()
    };

    assert_eq!(
        keys(read.range("t", b"b".as_slice()..b"d".as_slice()).unwrap()),
        [b"b", b"c"]
    );
    assert_eq!(
        keys(read.range("t", b"b".as_slice()..=b"d".as_slice()).unwrap()),
        [b"b", b"c", b"d"]
    );
    assert_eq!(
        keys(read.range("t", b"c".to_vec()..).unwrap()),
        [b"c", b"d"]
    );
    assert_eq!(keys(read.range("t", ..b"b".as_slice()).unwrap()), [b"a"]);
    assert!(keys(read.iter("missing").unwrap()).is_empty());
}

#[test]
fn deleting_a_tree_removes_it_and_everything_in_it() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    for index in 0..1000u32 {
        txn.insert("big", &index.to_be_bytes(), &[1; 64]).unwrap();
    }

    txn.insert("kept", b"k", b"v").unwrap();
    txn.commit().unwrap();

    let mut txn = db.begin_write().unwrap();

    assert!(txn.delete_tree("big").unwrap());
    assert!(!txn.delete_tree("never").unwrap());
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();

    assert_eq!(read.tree_names().unwrap(), ["kept"]);
    assert_eq!(read.len("big").unwrap(), 0);
    drop(read);

    // A tree created again under the same name starts empty.
    let mut txn = db.begin_write().unwrap();

    txn.insert("big", b"fresh", b"1").unwrap();
    txn.commit().unwrap();

    assert_eq!(entries(&db, "big"), [(b"fresh".to_vec(), b"1".to_vec())]);
}

#[test]
fn keys_and_tree_names_that_do_not_fit_are_refused() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    assert_eq!(
        txn.insert("t", &[0; 958], b"").unwrap_err().code(),
        "INVALID_ARGUMENT"
    );

    // A failed operation spoils the transaction: it can only be aborted.
    assert_eq!(txn.commit().unwrap_err().code(), "INVALID_ARGUMENT");

    let mut txn = db.begin_write().unwrap();

    txn.insert("t", &[0; 957], b"the longest key").unwrap();

    for name in ["", "\0engine"] {
        assert_eq!(txn.get(name, b"k").unwrap_err().code(), "INVALID_ARGUMENT");
    }
}

#[test]
fn every_handle_to_one_file_in_a_process_shares_one_database() {
    let dir = TestDir::new();
    let first = open(&dir);
    let second = open(&dir);
    let mut txn = first.begin_write().unwrap();

    txn.insert("t", b"k", b"v").unwrap();
    txn.commit().unwrap();

    assert_eq!(
        second.begin_read().unwrap().get("t", b"k").unwrap(),
        Some(b"v".to_vec())
    );
}

#[test]
fn a_second_writer_waits_and_then_gives_up_with_busy() {
    let dir = TestDir::new();
    let db = OpenOptions::new()
        .busy_timeout(Duration::from_millis(100))
        .open(dir.path("app.darudb"))
        .unwrap();
    let held = db.begin_write().unwrap();
    let other = db.clone();
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        sender.send(other.begin_write().map(|_| ())).unwrap();
    });

    let result = receiver.recv().unwrap();

    assert_eq!(result.unwrap_err().code(), "BUSY");

    drop(held);

    assert!(
        db.begin_write().is_ok(),
        "free again once the first is done"
    );
}

#[test]
fn many_commits_leave_a_file_that_opens_with_all_of_them() {
    let dir = TestDir::new();
    let db = open(&dir);

    for round in 0..50u32 {
        let mut txn = db.begin_write().unwrap();

        txn.insert("counter", b"value", &round.to_be_bytes())
            .unwrap();
        txn.insert("log", &round.to_be_bytes(), b"entry").unwrap();

        if round % 7 == 0 {
            txn.remove("log", &(round / 2).to_be_bytes()).unwrap();
        }

        txn.commit().unwrap();
    }

    drop(db);

    let db = open(&dir);
    let read = db.begin_read().unwrap();

    assert_eq!(
        read.get("counter", b"value").unwrap(),
        Some(49u32.to_be_bytes().to_vec())
    );
    // Rounds 0, 7, 14 and so on up to 49 each removed one entry: eight in all.
    assert_eq!(read.len("log").unwrap(), 50 - 8);
}

const PROBE_PATH: &str = "DARUDB_PROBE_PATH";

/// Run by [`unsynced`] in a process of its own: prints the selector byte of
/// the file named by `DARUDB_PROBE_PATH`. Run as an ordinary test, it has
/// nothing to do.
#[test]
fn selector_probe() {
    let Ok(path) = env::var(PROBE_PATH) else {
        return;
    };

    println!("\nselector {}", common::read(Path::new(&path))[64]);
}

/// Whether the selector byte of the file at `path` has bit 2 set, as it does
/// while deferred commits wait for a barrier (`design/file-format.md`).
///
/// Another process reads the file. This one has it open, and opening and
/// closing a second descriptor of it here would release the locks the
/// database holds on it.
fn unsynced(path: &Path) -> bool {
    let output = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "selector_probe",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(PROBE_PATH, path)
        .output()
        .unwrap();
    let selector: u8 = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("selector "))
        .unwrap()
        .parse()
        .unwrap();

    selector & 0b100 != 0
}

#[test]
fn a_deferred_commit_is_seen_at_once_and_made_durable_by_sync() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = OpenOptions::new()
        .max_unsynced_time(Duration::from_secs(3600))
        .open(&path)
        .unwrap();

    for round in 0..3u8 {
        let mut txn = db.begin_write().unwrap();

        txn.insert("t", &[round], b"deferred").unwrap();
        txn.commit_deferred().unwrap();
    }

    assert_eq!(entries(&db, "t").len(), 3, "readers see them at once");
    assert!(unsynced(&path));

    db.sync().unwrap();

    assert!(!unsynced(&path));

    db.close().unwrap();

    let db = open(&dir);

    assert_eq!(entries(&db, "t").len(), 3);
}

#[test]
fn a_sync_commit_makes_the_deferred_ones_before_it_durable() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = OpenOptions::new()
        .max_unsynced_time(Duration::from_secs(3600))
        .open(&path)
        .unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"a", b"deferred").unwrap();
    txn.commit_deferred().unwrap();

    assert!(unsynced(&path));

    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"b", b"sync").unwrap();
    txn.commit().unwrap();

    assert!(!unsynced(&path));
}

#[test]
fn the_unsynced_window_ends_on_its_own_when_its_time_is_up() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = OpenOptions::new()
        .max_unsynced_time(Duration::from_millis(20))
        .open(&path)
        .unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"a", b"deferred").unwrap();
    txn.commit_deferred().unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);

    while unsynced(&path) {
        assert!(
            std::time::Instant::now() < deadline,
            "the window never ended"
        );
        thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(entries(&db, "t").len(), 1);
}

#[test]
fn a_deferred_commit_past_the_page_limit_is_made_durable() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = OpenOptions::new()
        .max_unsynced_pages(1)
        .max_unsynced_time(Duration::from_secs(3600))
        .open(&path)
        .unwrap();
    let mut txn = db.begin_write().unwrap();

    // A value this large spans pages of its own, so the commit writes more
    // than one page.
    txn.insert("t", b"big", &vec![1; 20_000]).unwrap();
    txn.commit_deferred().unwrap();

    assert!(!unsynced(&path));
}

#[test]
fn dropping_the_last_handle_makes_deferred_commits_durable() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = OpenOptions::new()
        .max_unsynced_time(Duration::from_secs(3600))
        .open(&path)
        .unwrap();
    let other = db.clone();
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"a", b"deferred").unwrap();
    txn.commit_deferred().unwrap();
    drop(db);

    assert!(unsynced(&path), "another handle is still open");

    drop(other);

    assert!(!unsynced(&path));
}

#[test]
fn a_backward_range_gives_the_keys_from_the_last_down() {
    let dir = TestDir::new();
    let db = open(&dir);
    let mut txn = db.begin_write().unwrap();

    for key in [b"a", b"b", b"c", b"d"] {
        txn.insert("t", key, b"").unwrap();
    }

    txn.commit().unwrap();

    let keys = |range: darudb::Range<'_>| -> Vec<Vec<u8>> {
        range.map(|entry| entry.unwrap().0).collect()
    };
    let read = db.begin_read().unwrap();

    assert_eq!(
        keys(read.range_backward::<&[u8]>("t", ..).unwrap()),
        [b"d", b"c", b"b", b"a"]
    );
    assert_eq!(
        keys(
            read.range_backward("t", b"b".as_slice()..b"d".as_slice())
                .unwrap()
        ),
        [b"c", b"b"]
    );

    // A write transaction sees its own changes backwards too.
    let mut txn = db.begin_write().unwrap();

    txn.insert("t", b"e", b"").unwrap();
    txn.remove("t", b"a").unwrap();

    assert_eq!(
        keys(txn.range_backward::<&[u8]>("t", ..).unwrap()),
        [b"e", b"d", b"c", b"b"]
    );
}
