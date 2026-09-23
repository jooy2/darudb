//! Real processes killed while they write: `SIGKILL` on Unix-like systems,
//! `TerminateProcess` on Windows.
//!
//! The test binary runs itself as the child. The child commits in a loop and
//! prints each transaction id it has committed; the parent kills it at a
//! random moment, opens the file, and checks that every commit the child
//! reported is there. Without a power cut, the operating system still holds
//! everything the child wrote, so not even the commit in flight may be torn.
//!
//! `DARUDB_KILL_ROUNDS` sets how many times the child is killed; the default
//! keeps the suite quick, and a longer run is one environment variable away.

// A test and its child fail by panicking, helpers included.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::env;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::TestDir;
use darudb::Database;

const CHILD_PATH: &str = "DARUDB_KILL_CHILD_PATH";

/// The child: commits forever, printing the round of each commit.
#[test]
fn child_writer() {
    let Ok(path) = env::var(CHILD_PATH) else {
        // Run as an ordinary test, it has nothing to do.
        return;
    };
    let db = Database::open(&path).unwrap();
    let start = db
        .begin_read()
        .unwrap()
        .get("counter", b"round")
        .unwrap()
        .map_or(0, |bytes| u64::from_be_bytes(bytes.try_into().unwrap()) + 1);

    let mut round = start;

    // Until the parent kills this process.
    loop {
        let mut txn = db.begin_write().unwrap();
        let size = if round % 5 == 0 { 9000 } else { 40 };
        let fill = u8::try_from(round % 251).unwrap();

        txn.insert("counter", b"round", &round.to_be_bytes())
            .unwrap();
        txn.insert("log", &round.to_be_bytes(), &vec![fill; size])
            .unwrap();

        if round % 3 == 0 && round > 0 {
            txn.remove("log", &(round - 1).to_be_bytes()).unwrap();
        }

        txn.commit().unwrap();
        println!("{round}");
        round += 1;
    }
}

#[test]
fn a_killed_writer_never_loses_a_commit_it_reported() {
    let rounds: u64 = env::var("DARUDB_KILL_ROUNDS")
        .ok()
        .and_then(|rounds| rounds.parse().ok())
        .unwrap_or(12);
    let dir = TestDir::new();
    let path = dir.path("killed.darudb");

    for iteration in 0..rounds {
        let mut child = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "child_writer",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env(CHILD_PATH, &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(round) = line.trim().parse::<u64>() {
                    if sender.send(round).is_err() {
                        break;
                    }
                }
            }
        });

        // Let it commit a few times, then kill it somewhere in the middle of
        // whatever it is doing.
        let deadline = Instant::now() + Duration::from_millis(40 + (iteration * 37) % 160);
        let mut reported = receiver
            .recv_timeout(Duration::from_secs(20))
            .expect("the child never committed");

        while Instant::now() < deadline {
            if let Ok(round) = receiver.recv_timeout(Duration::from_millis(5)) {
                reported = round;
            }
        }

        child.kill().unwrap();
        child.wait().unwrap();

        // Anything printed before the kill counts as reported.
        while let Ok(round) = receiver.try_recv() {
            reported = round;
        }

        let db = Database::open(&path).unwrap();
        let read = db.begin_read().unwrap();
        let found = read
            .get("counter", b"round")
            .unwrap()
            .map(|bytes| u64::from_be_bytes(bytes.try_into().unwrap()))
            .unwrap();

        assert!(
            found == reported || found == reported + 1,
            "iteration {iteration}: the child reported round {reported}, the file holds {found}"
        );

        // Every page of the log is read back through its checks.
        let entries = read
            .iter("log")
            .unwrap()
            .collect::<darudb::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(entries.len() as u64, read.len("log").unwrap());
        assert!(
            entries
                .iter()
                .any(|(key, _)| key.as_slice() == found.to_be_bytes())
        );
    }
}
