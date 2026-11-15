//! The file itself: a small database with bytes changed anywhere opens or
//! is refused, and what opens is read, checked, written and salvaged.
//!
//! A fuzzing build gives every page one check (`Check::of`), so a changed
//! page still passes for the one its parent names: the input is a file
//! whose author worked the checks out again, which a plain file allows.
#![no_main]

use std::fs;
use std::sync::OnceLock;

use darudb::{Database, Filter, Object, OpenOptions, Query};
use libfuzzer_sys::fuzz_target;

/// The bytes of a database with a few objects, written in one transaction.
fn seed() -> &'static [u8] {
    static SEED: OnceLock<Vec<u8>> = OnceLock::new();

    SEED.get_or_init(|| {
        let path = darudb_fuzz::directory().join("seed.darudb");
        let db = OpenOptions::new()
            .schema(darudb_fuzz::schema())
            .open(&path)
            .expect("the seed opens");

        darudb_fuzz::fill(&db, 12).expect("the seed fills");
        db.close().expect("the seed closes");
        fs::read(&path).expect("the seed is read")
    })
}

fuzz_target!(|input: &[u8]| {
    let mut image = seed().to_vec();

    // Each four bytes of the input change one byte of the file: three say
    // where, the fourth what.
    for change in input.chunks_exact(4) {
        let at =
            usize::from(change[0]) << 16 | usize::from(change[1]) << 8 | usize::from(change[2]);
        let at = at % image.len();

        image[at] = change[3];
    }

    let directory = darudb_fuzz::directory();
    let path = directory.join("changed.darudb");
    let salvaged = directory.join("salvaged.darudb");

    fs::write(&path, &image).expect("the file is written");

    // With no schema, the file's own is read; with one, it is compared.
    for declared in [false, true] {
        let mut options = OpenOptions::new();

        if declared {
            options.schema(darudb_fuzz::schema());
        }

        let Ok(db) = options.open(&path) else {
            continue;
        };

        exercise(&db);

        let _ = db.close();
    }

    let _ = fs::remove_file(&salvaged);
    let _ = OpenOptions::new().salvage(&path, &salvaged);
});

/// Reads, checks and writes `db`, whatever state its file is in.
fn exercise(db: &Database) {
    let _ = db.check();

    if let Ok(read) = db.begin_read() {
        if let Ok(names) = read.tree_names() {
            for name in names {
                if let Ok(entries) = read.iter(&name) {
                    for entry in entries.take(1000) {
                        if entry.is_err() {
                            break;
                        }
                    }
                }
            }
        }

        if let Ok(people) = read.collection("people") {
            let _ = people.get(3);
            let _ = people.query(&Query::new());
            let _ = people.query(&Query::new().filter(Filter::ge("age", 3)).sort_by("name"));
            let _ = people.count(&Query::new().filter(Filter::eq("tags", "all")));
            let _ = people.sum(&Query::new(), "age");
        }
    }

    if let Ok(mut txn) = db.begin_write() {
        if let Ok(mut people) = txn.collection("people") {
            let _ = people.insert(Object::new().with("name", "new").with("age", 9));
            let _ = people.update(2, Object::new().with("age", 5));
            let _ = people.delete(1);
            let _ = people.delete_where(&Query::new().filter(Filter::eq("age", 4)));
        }

        let _ = txn.commit_deferred();
    }

    let _ = db.compact();
}
