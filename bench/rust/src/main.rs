//! The Rust side of the benchmark of `bench/README.md`: DaruDB and the
//! embedded stores Rust programs use most, each running the same workloads.
//!
//!   darudb-bench --stores                   the stores, with their versions, as JSON
//!   darudb-bench --child STORE --dir DIR    one pass of one store, one line of JSON per row
//!
//! `bench/run.mjs` runs the passes, each in a process of its own on new
//! files, and puts the runs together.

mod daru;
mod lmdb;
mod redb;
mod sqlite;
mod work;

use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let args: Vec<String> = env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .map(|at| args[at + 1].clone())
    };

    if args.iter().any(|arg| arg == "--stores") {
        println!(
            "[{{\"id\":\"daru\",\"version\":\"{}\"}},{{\"id\":\"sqlite\",\"version\":\"{} (rusqlite 0.40.2)\"}},{{\"id\":\"lmdb\",\"version\":\"heed 0.22.1\"}},{{\"id\":\"redb\",\"version\":\"4.3.0\"}}]",
            darudb::VERSION,
            rusqlite::version()
        );
        return;
    }

    let store = value("--child").expect("--child STORE");
    let dir = PathBuf::from(value("--dir").expect("--dir DIR"));
    let mut rows = work::Rows::default();

    fs::create_dir_all(&dir).expect("a directory");

    match store.as_str() {
        "daru" => daru::run(&dir, &mut rows).expect("daru"),
        "sqlite" => sqlite::run(&dir, &mut rows).expect("sqlite"),
        "lmdb" => lmdb::run(&dir, &mut rows).expect("lmdb"),
        "redb" => redb::run(&dir, &mut rows).expect("redb"),
        other => panic!("no store is named {other}"),
    }

    rows.print();
}
