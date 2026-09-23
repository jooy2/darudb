//! Measures the storage kernel on this machine: commits, bulk writes, reads,
//! large values, and commits over fragmented free space.
//!
//! ```text
//! cargo run -p darudb --release --example kernel_bench [directory]
//! ```
//!
//! The files go into `directory`, the system's temporary directory by
//! default, and are removed afterwards. The numbers are for comparing one
//! build of the kernel with another on the same machine; they say nothing
//! about another machine or another file system.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use darudb::{Database, OpenOptions};

/// Keys in the bulk and read workloads.
const KEYS: u64 = 200_000;
const VALUE: [u8; 100] = [7; 100];

type Outcome = Result<(), Box<dyn Error>>;

fn main() -> Outcome {
    let directory = env::args()
        .nth(1)
        .map_or_else(env::temp_dir, PathBuf::from)
        .join(format!("darudb-bench-{}", process::id()));

    fs::create_dir_all(&directory)?;

    let outcome = run(&directory);

    fs::remove_dir_all(&directory)?;

    outcome
}

fn run(directory: &Path) -> Outcome {
    println!("{:<44} {:>12} {:>14}", "workload", "per second", "each");

    let db = open(directory, "commits.darudb")?;

    measure("sync commit, one 100-byte value", 500, |round| {
        let mut txn = db.begin_write()?;

        txn.insert("t", &round.to_be_bytes(), &VALUE)?;
        txn.commit()
    })?;
    measure("deferred commit, one 100-byte value", 20_000, |round| {
        let mut txn = db.begin_write()?;

        txn.insert("t", &(1_000_000 + round).to_be_bytes(), &VALUE)?;
        txn.commit_deferred()
    })?;
    db.close()?;

    let db = open(directory, "bulk.darudb")?;
    let mut bulk = Some(db.begin_write()?);

    measure(
        "insert in one transaction, 100-byte values",
        KEYS,
        |key| match &mut bulk {
            Some(txn) => txn.insert("t", &scatter(key).to_be_bytes(), &VALUE),
            None => Ok(()),
        },
    )?;
    measure("commit of that transaction", 1, |_| {
        bulk.take().map_or(Ok(()), darudb::WriteTransaction::commit)
    })?;

    let read = db.begin_read()?;

    measure("get, random order", KEYS, |key| {
        read.get("t", &scatter(key).to_be_bytes()).map(drop)
    })?;
    measure("iterate every entry", 1, |_| {
        read.iter("t")?.try_for_each(|entry| entry.map(drop))
    })?;
    drop(read);

    // Every other block of neighbouring keys goes, which empties every other
    // leaf and leaves the free space in many small runs.
    let keys: Vec<Vec<u8>> = db
        .begin_read()?
        .iter("t")?
        .map(|entry| entry.map(|(key, _)| key))
        .collect::<darudb::Result<_>>()?;
    let mut txn = db.begin_write()?;

    for block in keys.chunks(64).step_by(2) {
        for key in block {
            txn.remove("t", key)?;
        }
    }

    txn.commit()?;
    measure(
        "deferred commit over fragmented free space",
        5_000,
        |round| {
            let mut txn = db.begin_write()?;

            txn.insert("u", &round.to_be_bytes(), &VALUE)?;
            txn.commit_deferred()
        },
    )?;
    db.close()?;

    let db = open(directory, "large.darudb")?;
    let large = vec![3u8; 256 * 1024];

    measure("deferred commit, one 256 KiB value", 400, |round| {
        let mut txn = db.begin_write()?;

        txn.insert("t", &round.to_be_bytes(), &large)?;
        txn.commit_deferred()
    })?;

    let read = db.begin_read()?;

    measure("get of a 256 KiB value", 400, |round| {
        read.get("t", &round.to_be_bytes()).map(drop)
    })?;

    Ok(())
}

fn open(directory: &Path, name: &str) -> Result<Database, darudb::Error> {
    OpenOptions::new().open(directory.join(name))
}

/// Runs `step` `count` times and prints how fast it went.
fn measure(
    name: &str,
    count: u64,
    mut step: impl FnMut(u64) -> darudb::Result<()>,
) -> Result<(), darudb::Error> {
    let started = Instant::now();

    for round in 0..count {
        step(round)?;
    }

    let elapsed = started.elapsed();
    let seconds = elapsed.as_secs_f64();
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts stay far below 2^52, where f64 is exact"
    )]
    let rate = count as f64 / seconds;
    let each = elapsed.checked_div(u32::try_from(count).unwrap_or(u32::MAX));

    println!(
        "{name:<44} {rate:>12.0} {:>14}",
        format_duration(each.unwrap_or(Duration::ZERO))
    );

    Ok(())
}

fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos >= 1_000_000 {
        format!("{:.2} ms", duration.as_secs_f64() * 1e3)
    } else if nanos >= 1_000 {
        format!("{:.2} us", duration.as_secs_f64() * 1e6)
    } else {
        format!("{nanos} ns")
    }
}

/// Spreads consecutive numbers over the key space, so that inserts and reads
/// land all over the tree instead of at its end.
fn scatter(key: u64) -> u64 {
    key.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17)
}
