//! The Rust side of the benchmark of `bench/README.md`: DaruDB and the
//! embedded stores Rust programs use most, each running the same workloads.
//!
//!   darudb-bench --stores [--suite S]                   the stores, with their versions, as JSON
//!   darudb-bench --child STORE --dir DIR [--suite S]    one pass of one store, one line of JSON per row
//!
//! The suite is `simple`, by default, or `complex`, which only the stores
//! with a query language run. `bench/run.mjs` runs the passes, each in a
//! process of its own on new files, and puts the runs together.

mod complex;
mod complex_daru;
mod complex_sqlite;
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
    let complex = match value("--suite").as_deref() {
        None | Some("simple") => false,
        Some("complex") => true,
        Some(other) => panic!("no suite is named {other}"),
    };

    if args.iter().any(|arg| arg == "--stores") {
        let daru = format!("{{\"id\":\"daru\",\"version\":\"{}\"}}", darudb::VERSION);
        let sqlite = format!(
            "{{\"id\":\"sqlite\",\"version\":\"{} (rusqlite 0.40.2)\"}}",
            rusqlite::version()
        );

        if complex {
            println!("[{daru},{sqlite}]");
        } else {
            println!(
                "[{daru},{sqlite},{{\"id\":\"lmdb\",\"version\":\"heed 0.22.1\"}},{{\"id\":\"redb\",\"version\":\"4.3.0\"}}]"
            );
        }
        return;
    }

    let store = value("--child").expect("--child STORE");
    let dir = PathBuf::from(value("--dir").expect("--dir DIR"));
    let mut rows = work::Rows::default();

    fs::create_dir_all(&dir).expect("a directory");

    match (complex, store.as_str()) {
        (false, "daru") => daru::run(&dir, &mut rows).expect("daru"),
        (false, "sqlite") => sqlite::run(&dir, &mut rows).expect("sqlite"),
        (false, "lmdb") => lmdb::run(&dir, &mut rows).expect("lmdb"),
        (false, "redb") => redb::run(&dir, &mut rows).expect("redb"),
        (true, "daru") => complex_daru::run(&dir, &mut rows).expect("daru"),
        (true, "sqlite") => complex_sqlite::run(&dir, &mut rows).expect("sqlite"),
        (_, other) => panic!("no store is named {other} in this suite"),
    }

    rows.print();
}
