//! The locks between real processes. Each test runs a helper, another process
//! of this test binary, because a process never conflicts with its own locks
//! on Unix-like systems.

use std::env;
use std::thread;
use std::time::Duration;

use crate::testing::{HELPER_PATH, Helper};
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
