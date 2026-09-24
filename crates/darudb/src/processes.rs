//! The multi-process suite: several processes share one file, reading and
//! writing at random, while random ones are killed and new ones take their
//! place. It is the phase 3 exit criterion that `design/locking.md` spells out
//! under "What the phase 3 tests must show".
//!
//! Each worker is another process of this test binary. It opens the file
//! through two handles and runs three threads: one commits transfers between
//! accounts, sync and deferred commits at random, and two begin read
//! transactions, some of them held for a while, and now and then close their
//! handle and open it again. Every commit moves money between accounts, so
//! the accounts always add up to [`TOTAL`], and it records the worker's
//! sequence number, which the worker prints once the commit returns. Values of
//! every size, some spanning pages of their own, come and go in a log.
//!
//! What must hold, and where it is checked:
//!
//! - **Every snapshot is one commit.** Every reader checks the total, and a
//!   reader that holds its snapshot while others commit reads the same
//!   contents twice.
//! - **No page is reused under a live snapshot.** A page that fails its check
//!   in a reader fails the run: it can only have been overwritten.
//! - **Nothing is lost.** Once every worker has stopped, the file passes the
//!   integrity check and holds the last commit every worker reported, deferred
//!   ones included, since no power was cut.
//! - **Dead processes hold nothing back.** Once they have stopped, a few
//!   commits reclaim every page their snapshots kept.
//! - **One process, many handles.** Closing and opening handles in a worker
//!   never releases a lock another of its threads relies on; if it did, the
//!   readers above would see pages reused. One worker in three keeps no handle
//!   for long in any thread, so that its instance closes, often while another
//!   of its threads opens the file again.
//!
//! Half the workers release a snapshot lock with its last reader rather than
//! keep it for the next one to join, so that their readers register snapshots
//! afresh; the other half keep them, and test the joining. The window between
//! a reader's first read of the header and its registration is too short for
//! a suite this size to hit; `lock/tests.rs` stops a reader in it on purpose.
//!
//! `DARUDB_PROCESS_KILLS` sets how many workers are killed in each of the two
//! runs, one on a plain file and one on an encrypted file. The default keeps
//! the suite quick, and a longer run is one environment variable away.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::io::{self, BufRead};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::btree::{self, Child};
use crate::format::{RETAINED_TREE, decode_retained_key};
use crate::testing::{Helper, Rng};
use crate::{Database, OpenOptions, ReadTransaction};

/// How many accounts the transfers move money between.
const ACCOUNTS: u64 = 48;

/// What each account starts with.
const OPENING_BALANCE: u64 = 1000;

/// What the accounts add up to in every commit.
const TOTAL: u64 = ACCOUNTS * OPENING_BALANCE;

/// How many workers run at once.
const WORKERS: usize = 4;

/// How many entries the log keeps, roughly: a writer removes the oldest ones
/// past this, which keeps pages moving between used, retained and free.
const LOG_ENTRIES: u64 = 150;

const WORKER_PATH: &str = "DARUDB_WORKER_PATH";
const WORKER_ID: &str = "DARUDB_WORKER_ID";

/// The options every process opens the file with. An encrypted file's name
/// says so. Busy timeouts are long, so that only a writer that starves or a
/// lock that is never released fails with `BUSY`.
fn options(path: &Path, rng: &mut Rng) -> OpenOptions {
    let mut options = OpenOptions::new();

    options
        .busy_timeout(Duration::from_secs(60))
        .max_unsynced_time(Duration::from_millis(5 + rng.below(200)))
        .max_unsynced_pages(16 + rng.below(1000));

    if path.to_string_lossy().contains("encrypted") {
        options.key([0x6E; 32]);
    }

    options
}

/// Reports a failure to the parent, which fails the run, and ends the worker.
fn fail(message: &str) -> ! {
    println!("failed {message}");
    std::process::exit(1)
}

/// The worker: runs until its input closes or it is killed. A panic anywhere
/// in it, in any thread, is a failure too.
#[test]
fn worker() {
    let (Ok(path), Ok(id)) = (env::var(WORKER_PATH), env::var(WORKER_ID)) else {
        // Run as an ordinary test, it has nothing to do.
        return;
    };
    let id: u64 = id.parse().unwrap();

    std::panic::set_hook(Box::new(|info| fail(&info.to_string())));

    let path = Path::new(&path);
    let mut rng = Rng::new(id);
    let options = options(path, &mut rng);

    crate::testing::KEEP_SNAPSHOT_LOCKS.store(id % 2 == 1, Ordering::Relaxed);

    let churn = id % 3 == 0;
    let stop = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..2)
        .map(|reader| {
            let stop = Arc::clone(&stop);
            let options = options.clone();
            let path = path.to_path_buf();
            let seed = id.wrapping_mul(31).wrapping_add(reader);

            thread::spawn(move || read(&path, &options, churn, &stop, seed))
        })
        .collect();

    {
        let stop = Arc::clone(&stop);

        // The parent closes the input to stop the worker.
        thread::spawn(move || {
            for _ in io::stdin().lock().lines() {}

            stop.store(true, Ordering::SeqCst);
        });
    }

    let db = write(path, &options, churn, id, &stop, &mut rng);

    for reader in readers {
        let _ = reader.join();
    }

    db.close()
        .unwrap_or_else(|error| fail(&format!("closing: {error}")));
    println!("stopped");
}

/// Opens the database, and fails the worker if that fails.
fn open(path: &Path, options: &OpenOptions) -> Database {
    options
        .open(path)
        .unwrap_or_else(|error| fail(&format!("opening: {error}")))
}

/// Commits transfers until told to stop, printing each sequence number once
/// its commit has returned. With `churn`, it opens a new handle every few
/// transactions and drops the old one first. Returns its last handle.
fn write(
    path: &Path,
    options: &OpenOptions,
    churn: bool,
    id: u64,
    stop: &AtomicBool,
    rng: &mut Rng,
) -> Database {
    let mut handle = None;
    let mut sequence = 0u64;

    while !stop.load(Ordering::SeqCst) {
        if churn && rng.below(4) == 0 {
            handle = None;
        }

        let db: &Database = handle.get_or_insert_with(|| open(path, options));
        let result = (|| {
            let mut txn = db.begin_write()?;
            let from = rng.below(ACCOUNTS);
            let to = rng.below(ACCOUNTS);
            let balance = |txn: &crate::WriteTransaction, account: u64| {
                txn.get("accounts", &account.to_be_bytes())
                    .map(|value| u64::from_be_bytes(value.unwrap().try_into().unwrap()))
            };
            let amount = rng.below(balance(&txn, from)? + 1);

            txn.insert(
                "accounts",
                &from.to_be_bytes(),
                &(balance(&txn, from)? - amount).to_be_bytes(),
            )?;
            txn.insert(
                "accounts",
                &to.to_be_bytes(),
                &(balance(&txn, to)? + amount).to_be_bytes(),
            )?;
            txn.insert("writers", &id.to_be_bytes(), &sequence.to_be_bytes())?;

            // A value from a few bytes to several pages long.
            let len = match rng.below(10) {
                0 => rng.index(40_000),
                1..=3 => rng.index(2_000),
                _ => rng.index(100),
            };
            let key = [id.to_be_bytes(), sequence.to_be_bytes()].concat();

            txn.insert("log", &key, &rng.bytes(len))?;

            if txn.len("log")? > LOG_ENTRIES {
                let oldest: Vec<Vec<u8>> = txn
                    .iter("log")?
                    .take(1 + rng.index(4))
                    .map(|entry| entry.map(|(key, _)| key))
                    .collect::<crate::Result<_>>()?;

                for key in oldest {
                    txn.remove("log", &key)?;
                }
            }

            match rng.below(10) {
                0 => {
                    txn.abort();

                    Ok(false)
                }
                1..=3 => txn.commit().map(|()| true),
                _ => txn.commit_deferred().map(|()| true),
            }
        })();

        match result {
            Ok(true) => {
                println!("committed {sequence}");
                sequence += 1;
            }
            Ok(false) => {}
            Err(error) => fail(&format!("writing: {error}")),
        }

        if rng.below(40) == 0 {
            db.sync()
                .unwrap_or_else(|error| fail(&format!("syncing: {error}")));
        }
    }

    handle.unwrap_or_else(|| open(path, options))
}

/// Reads until told to stop, through a handle of its own that it drops and
/// opens again now and then, or with `churn` after every read.
fn read(path: &Path, options: &OpenOptions, churn: bool, stop: &AtomicBool, seed: u64) {
    let mut rng = Rng::new(seed);
    let mut handle = None;

    while !stop.load(Ordering::SeqCst) {
        let db: &Database = handle.get_or_insert_with(|| open(path, options));
        let txn = db
            .begin_read()
            .unwrap_or_else(|error| fail(&format!("beginning a read: {error}")));
        let first = contents(&txn).unwrap_or_else(|message| fail(&message));

        // Held while others commit, the snapshot must not change.
        if rng.below(3) == 0 {
            thread::sleep(Duration::from_millis(rng.below(40)));

            let second = contents(&txn).unwrap_or_else(|message| fail(&message));

            if second != first {
                fail(&format!(
                    "snapshot {} read differently the second time",
                    txn.commit_id()
                ));
            }
        }

        drop(txn);

        if churn || rng.below(30) == 0 {
            handle = None;
        }
    }
}

/// What a snapshot holds, reduced to a few numbers, after checking that its
/// accounts add up. Every page of it is read, and so verified.
fn contents(txn: &ReadTransaction) -> Result<(u64, u64, u64), String> {
    let failed = |error: crate::Error| format!("reading snapshot {}: {error}", txn.commit_id());
    let mut total = 0;

    for entry in txn.iter("accounts").map_err(failed)? {
        let (_, value) = entry.map_err(failed)?;

        total += u64::from_be_bytes(value.try_into().unwrap());
    }

    if total != TOTAL {
        return Err(format!(
            "snapshot {} holds {total} in its accounts, not {TOTAL}",
            txn.commit_id()
        ));
    }

    let mut digest = 0u64;
    let mut entries = 0;

    for entry in txn.iter("log").map_err(failed)? {
        let (key, value) = entry.map_err(failed)?;

        entries += 1;

        for byte in key.iter().chain(&value) {
            digest = (digest ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01B3);
        }
    }

    Ok((txn.commit_id(), entries, digest))
}

/// A worker, and the last sequence number it reported.
struct Running {
    id: u64,
    helper: Helper,
    reported: Option<u64>,
}

impl Running {
    fn start(path: &Path, id: u64) -> Self {
        let id_text = id.to_string();
        let helper = Helper::spawn_with(
            "processes::worker",
            &[
                (WORKER_PATH, path.as_os_str()),
                (WORKER_ID, OsStr::new(&id_text)),
            ],
        );

        Self {
            id,
            helper,
            reported: None,
        }
    }

    /// Takes in what the worker printed, and fails the run on a failure.
    fn take(&mut self, lines: Vec<String>) {
        take(self.id, &mut self.reported, lines);
    }
}

/// Takes in what worker `id` printed into `reported`, the last sequence
/// number it reported, and fails the run on a failure it reported.
fn take(id: u64, reported: &mut Option<u64>, lines: Vec<String>) {
    for line in lines {
        if let Some(at) = line.find("failed ") {
            panic!("worker {id}: {}", &line[at..]);
        }

        if let Some(at) = line.find("committed ") {
            *reported = Some(line[at + "committed ".len()..].trim().parse().unwrap());
        }
    }
}

/// One run: workers on the file at `path` for `kills` kills, then every
/// check on what they left.
fn run(path: &Path, kills: u64, seed: u64) {
    let mut rng = Rng::new(seed);
    let db = options(path, &mut rng).open(path).unwrap();
    let mut txn = db.begin_write().unwrap();

    for account in 0..ACCOUNTS {
        txn.insert(
            "accounts",
            &account.to_be_bytes(),
            &OPENING_BALANCE.to_be_bytes(),
        )
        .unwrap();
    }

    txn.commit().unwrap();
    drop(db);

    let mut next_id = seed * 10_000;
    let mut running: Vec<Running> = (0..WORKERS)
        .map(|_| {
            next_id += 1;

            Running::start(path, next_id)
        })
        .collect();
    let mut stopped: BTreeMap<u64, (Option<u64>, bool)> = BTreeMap::new();

    for _ in 0..kills {
        for _ in 0..1 + rng.below(20) {
            thread::sleep(Duration::from_millis(10));

            for worker in &mut running {
                let lines = worker.helper.printed();

                worker.take(lines);
            }
        }

        let Running {
            id,
            helper,
            mut reported,
        } = running.swap_remove(rng.index(running.len()));

        take(id, &mut reported, helper.kill_and_read());
        stopped.insert(id, (reported, true));
        next_id += 1;
        running.push(Running::start(path, next_id));
    }

    // Every worker is told to stop before any is waited for, so that none
    // keeps the others waiting for the writer lock meanwhile.
    for worker in &mut running {
        worker.helper.close_input();
    }

    for Running {
        id,
        helper,
        mut reported,
    } in running
    {
        let (exited, lines) = helper.finish();

        take(id, &mut reported, lines);
        assert!(exited, "worker {id} did not stop cleanly");
        stopped.insert(id, (reported, false));
    }

    check(path, &stopped, &mut rng);
}

/// What the file must hold once every worker has stopped.
fn check(path: &Path, stopped: &BTreeMap<u64, (Option<u64>, bool)>, rng: &mut Rng) {
    let db = options(path, rng).open(path).unwrap();

    crate::crash::check_integrity(&db).unwrap();

    let read = db.begin_read().unwrap();

    contents(&read).unwrap();

    // A killed worker may have made one more commit than it printed.
    for (id, (reported, killed)) in stopped {
        let found = read
            .get("writers", &id.to_be_bytes())
            .unwrap()
            .map(|value| u64::from_be_bytes(value.try_into().unwrap()));
        let allowed = match (reported, killed) {
            (None, true) => found.is_none() || found == Some(0),
            (Some(last), true) => found == Some(*last) || found == Some(last + 1),
            (reported, false) => found == *reported,
        };

        assert!(
            allowed,
            "worker {id} reported {reported:?}, killed: {killed}, and the file holds {found:?}"
        );
    }

    drop(read);

    // The dead held no snapshot the living cannot reclaim: with nobody
    // reading, two commits leave only the group of the last one.
    for _ in 0..2 {
        let mut txn = db.begin_write().unwrap();

        txn.insert("writers", b"parent", b"").unwrap();
        txn.commit().unwrap();
    }

    let record = db.shared().header().published().unwrap();
    let root = (!record.retained.is_null()).then_some(Child::Clean(record.retained));
    let groups: Vec<u64> = btree::Range::new(
        &db.shared().loader,
        RETAINED_TREE,
        root.as_ref(),
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
    )
    .unwrap()
    .map(|entry| decode_retained_key(&entry.unwrap().0).unwrap().0)
    .collect();

    assert!(
        groups.iter().all(|group| *group == record.txn),
        "retained groups {groups:?} outlived commit {}",
        record.txn
    );

    crate::crash::check_integrity(&db).unwrap();
}

#[test]
fn processes_share_one_file_while_random_ones_are_killed() {
    let kills: u64 = env::var("DARUDB_PROCESS_KILLS")
        .ok()
        .and_then(|kills| kills.parse().ok())
        .unwrap_or(12);
    let dir = tempfile::tempdir().unwrap();

    run(&dir.path().join("plain.darudb"), kills, 1);
    run(&dir.path().join("encrypted.darudb"), kills, 2);
}
