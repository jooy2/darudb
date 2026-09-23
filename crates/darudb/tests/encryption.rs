//! Encrypted databases, through the public API only: keys, passwords, key
//! changes, and what an encrypted file lets through.

// The helpers below fail the test that called them by panicking, the way a
// test does; `clippy.toml` allows that inside tests but not in helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use common::TestDir;
use darudb::{Database, OpenOptions};

const KEY: [u8; 32] = [0x5A; 32];
const VALUE: &[u8] = b"a value no one else may read";

/// Options with a password hashing cost cheap enough for tests. A file
/// records its own cost, so reopening it costs the same with any options.
fn cheap() -> OpenOptions {
    let mut options = OpenOptions::new();

    options.password_hashing(8, 1, 1);
    options
}

fn with_key(key: [u8; 32]) -> OpenOptions {
    let mut options = OpenOptions::new();

    options.key(key);
    options
}

fn with_password(password: &str) -> OpenOptions {
    let mut options = cheap();

    options.password(password);
    options
}

fn write_value(db: &Database) {
    let mut txn = db.begin_write().unwrap();

    txn.insert("secrets", b"entry", VALUE).unwrap();
    txn.insert("secrets", b"large", &vec![0x33; 50_000])
        .unwrap();
    txn.commit().unwrap();
}

fn read_value(db: &Database) -> Option<Vec<u8>> {
    db.begin_read().unwrap().get("secrets", b"entry").unwrap()
}

fn code(result: darudb::Result<Database>) -> &'static str {
    result.unwrap_err().code()
}

fn contains(path: &Path, needle: &[u8]) -> bool {
    common::read(path)
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn an_encrypted_database_opens_with_its_key_and_nothing_else() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = with_key(KEY).open(&path).unwrap();

    assert!(db.is_encrypted());
    write_value(&db);
    db.close().unwrap();

    assert!(
        !contains(&path, VALUE),
        "the value is in the file in the clear"
    );
    assert!(
        !contains(&path, b"secrets"),
        "the tree name is in the clear"
    );
    assert!(
        !contains(&path, &[0x33; 64]),
        "the large value is in the clear"
    );

    assert_eq!(code(Database::open(&path)), "KEY_REQUIRED");
    assert_eq!(code(with_key([1; 32]).open(&path)), "WRONG_KEY");
    assert_eq!(code(with_password("guess").open(&path)), "WRONG_KEY");

    let db = with_key(KEY).open(&path).unwrap();

    assert_eq!(read_value(&db), Some(VALUE.to_vec()));
    assert_eq!(
        db.begin_read().unwrap().get("secrets", b"large").unwrap(),
        Some(vec![0x33; 50_000])
    );
}

#[test]
fn a_password_opens_the_database_it_encrypted() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = with_password("correct horse").open(&path).unwrap();

    write_value(&db);
    db.close().unwrap();

    assert_eq!(
        code(with_password("battery staple").open(&path)),
        "WRONG_KEY"
    );
    assert_eq!(code(with_key(KEY).open(&path)), "WRONG_KEY");

    // The default cost in the options does not matter: the file's does.
    let db = OpenOptions::new()
        .password("correct horse")
        .open(&path)
        .unwrap();

    assert_eq!(read_value(&db), Some(VALUE.to_vec()));
}

#[test]
fn changing_the_key_locks_out_the_old_one() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = with_password("old password").open(&path).unwrap();

    write_value(&db);
    db.set_password("new password").unwrap();

    // The handle stays open and usable across the change.
    assert_eq!(read_value(&db), Some(VALUE.to_vec()));
    db.close().unwrap();

    assert_eq!(code(with_password("old password").open(&path)), "WRONG_KEY");

    let db = with_password("new password").open(&path).unwrap();

    assert_eq!(read_value(&db), Some(VALUE.to_vec()));

    // From a password to a key, too.
    db.set_key(KEY).unwrap();
    db.close().unwrap();

    assert_eq!(code(with_password("new password").open(&path)), "WRONG_KEY");
    assert_eq!(
        read_value(&with_key(KEY).open(&path).unwrap()),
        Some(VALUE.to_vec())
    );
}

#[test]
fn a_plain_database_takes_no_key() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = Database::open(&path).unwrap();

    assert!(!db.is_encrypted());
    write_value(&db);

    assert_eq!(db.set_key(KEY).unwrap_err().code(), "INVALID_ARGUMENT");
    assert_eq!(code(with_key(KEY).open(&path)), "INVALID_ARGUMENT");

    db.close().unwrap();

    assert_eq!(code(with_key(KEY).open(&path)), "INVALID_ARGUMENT");
    assert!(contains(&path, VALUE), "a plain file is plain");
}

#[test]
fn another_handle_in_the_process_needs_the_key_as_well() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = with_key(KEY).open(&path).unwrap();

    write_value(&db);

    assert_eq!(code(Database::open(&path)), "KEY_REQUIRED");
    assert_eq!(code(with_key([1; 32]).open(&path)), "WRONG_KEY");
    assert_eq!(
        read_value(&with_key(KEY).open(&path).unwrap()),
        Some(VALUE.to_vec())
    );
}

#[test]
fn an_empty_password_is_refused() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    assert_eq!(code(with_password("").open(&path)), "INVALID_ARGUMENT");
    assert!(!path.exists());

    let db = with_password("something").open(&path).unwrap();

    assert_eq!(db.set_password("").unwrap_err().code(), "INVALID_ARGUMENT");
}

#[test]
fn a_changed_byte_anywhere_in_an_encrypted_page_is_reported() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = with_key(KEY).open(&path).unwrap();

    write_value(&db);
    db.close().unwrap();

    let original = common::read(&path);
    let pages = original.len() / 4096;

    for page in 1..pages {
        for at in [0, 30, 2000, 4095] {
            let offset = page * 4096 + at;

            common::patch(&path, offset, &[original[offset] ^ 0x80]);

            let outcome = with_key(KEY).open(&path).and_then(|db| {
                let read = db.begin_read()?;

                read.get("secrets", b"entry")?;
                read.get("secrets", b"large")?;
                read.len("secrets").map(drop)
            });

            if let Err(error) = outcome {
                assert_eq!(error.code(), "CORRUPTED", "page {page}, byte {at}: {error}");
            }

            common::patch(&path, offset, &[original[offset]]);
        }
    }

    assert_eq!(
        read_value(&with_key(KEY).open(&path).unwrap()),
        Some(VALUE.to_vec())
    );
}
