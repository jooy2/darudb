//! Opening and creating a database file, through the public API only.

mod common;

use std::fs;

use common::{TestDir, patch, read};
use darudb::{Database, Error, FORMAT_VERSION, OpenOptions};

#[test]
fn opening_a_missing_path_creates_a_database() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    let db = Database::open(&path).unwrap();

    assert_eq!(db.path(), path);
    assert_eq!(db.page_size(), 4096);
    assert_eq!(db.format_version(), FORMAT_VERSION);
    db.close().unwrap();

    // The first page is written whole, so the file is exactly one page long.
    assert_eq!(read(&path).len(), 4096);
}

#[test]
fn a_created_database_opens_again_with_what_it_recorded() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    OpenOptions::new()
        .page_size(16384)
        .open(&path)
        .unwrap()
        .close()
        .unwrap();

    let db = OpenOptions::new().create(false).open(&path).unwrap();

    assert_eq!(db.page_size(), 16384);
    assert_eq!(db.format_version(), FORMAT_VERSION);
}

#[test]
fn an_existing_database_keeps_its_page_size_whatever_the_options_ask() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    Database::open(&path).unwrap().close().unwrap();

    let db = OpenOptions::new().page_size(65536).open(&path).unwrap();

    assert_eq!(db.page_size(), 4096);
}

#[test]
fn a_missing_database_is_not_created_when_creating_is_off() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    let error = OpenOptions::new().create(false).open(&path).unwrap_err();

    assert!(matches!(error, Error::NotFound { .. }), "{error:?}");
    assert_eq!(error.code(), "NOT_FOUND");
    assert!(!path.exists());
}

#[test]
fn a_page_size_that_is_not_a_power_of_two_in_range_is_refused() {
    let dir = TestDir::new();

    for page_size in [0, 512, 2048, 1000, 131072] {
        let path = dir.path(&format!("app-{page_size}.darudb"));
        let error = OpenOptions::new()
            .page_size(page_size)
            .open(&path)
            .unwrap_err();

        assert_eq!(error.code(), "INVALID_ARGUMENT", "{page_size}");
        assert!(!path.exists(), "a file was created for {page_size}");
    }
}

#[test]
fn a_file_that_is_not_a_database_is_refused_and_left_alone() {
    let dir = TestDir::new();
    let text = b"a shopping list, not a database";
    let path = dir.file("notes.txt", text);

    let error = Database::open(&path).unwrap_err();

    assert_eq!(error.code(), "NOT_A_DATABASE");
    assert_eq!(read(&path), text);
}

#[test]
fn an_empty_file_is_refused_rather_than_turned_into_a_database() {
    // An empty file at the path is not a database waiting to be created. It
    // may be one that was truncated, and writing a header into it would hide
    // that for good.
    let dir = TestDir::new();
    let path = dir.file("app.darudb", b"");

    let error = Database::open(&path).unwrap_err();

    assert_eq!(error.code(), "NOT_A_DATABASE");
    assert!(read(&path).is_empty());
}

#[test]
fn a_database_in_another_format_version_is_refused_with_both_versions() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    Database::open(&path).unwrap().close().unwrap();
    patch(&path, 8, &(FORMAT_VERSION + 1).to_le_bytes());

    match Database::open(&path).unwrap_err() {
        Error::UnsupportedFormatVersion {
            found, supported, ..
        } => {
            assert_eq!(found, FORMAT_VERSION + 1);
            assert_eq!(supported, FORMAT_VERSION);
        }
        other => panic!("expected UNSUPPORTED_FORMAT_VERSION, got {other:?}"),
    }
}

#[test]
fn a_database_older_than_the_first_released_format_is_refused() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    Database::open(&path).unwrap().close().unwrap();
    patch(&path, 8, &4u32.to_le_bytes());

    match Database::open(&path).unwrap_err() {
        Error::UnsupportedFormatVersion { found, .. } => assert_eq!(found, 4),
        other => panic!("expected UNSUPPORTED_FORMAT_VERSION, got {other:?}"),
    }
}

#[test]
fn a_new_database_is_in_the_newest_format_unless_upgrading_is_off() {
    let dir = TestDir::new();
    let newest = dir.path("newest.darudb");
    let oldest = dir.path("oldest.darudb");

    assert_eq!(
        Database::open(&newest).unwrap().format_version(),
        FORMAT_VERSION
    );
    assert_eq!(
        OpenOptions::new()
            .upgrade_format(false)
            .open(&oldest)
            .unwrap()
            .format_version(),
        5
    );
    assert_eq!(read(&newest)[8], 6);
    assert_eq!(read(&oldest)[8], 5);
}

/// A file of format 5 stays in it while opening is told not to raise it,
/// and is raised to format 6 by opening it otherwise, or by the call, with
/// what it holds.
#[test]
fn a_file_of_format_5_is_raised_by_opening_it_or_by_the_call() {
    let dir = TestDir::new();
    let mut old = OpenOptions::new();

    old.upgrade_format(false);

    for (name, by_call) in [("opened.darudb", false), ("called.darudb", true)] {
        let path = dir.path(name);

        {
            let db = old.open(&path).unwrap();
            let mut txn = db.begin_write().unwrap();

            for n in 0..2_000u32 {
                txn.insert("t", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
            }

            txn.commit().unwrap();
        }

        let db = old.open(&path).unwrap();

        assert_eq!(db.format_version(), 5);
        assert_eq!(read(&path)[8], 5);

        let db = if by_call {
            assert!(db.upgrade_format().unwrap());
            db
        } else {
            drop(db);
            Database::open(&path).unwrap()
        };

        assert_eq!(db.format_version(), 6);
        assert_eq!(read(&path)[8], 6);

        let mut txn = db.begin_write().unwrap();

        txn.insert("t", &7u32.to_be_bytes(), b"changed").unwrap();
        txn.commit().unwrap();

        let read = db.begin_read().unwrap();

        assert_eq!(read.len("t").unwrap(), 2_000);
        assert_eq!(
            read.get("t", &7u32.to_be_bytes()).unwrap().as_deref(),
            Some(&b"changed"[..])
        );
        assert_eq!(
            read.get("t", &1_999u32.to_be_bytes()).unwrap(),
            Some(1_999u32.to_le_bytes().to_vec())
        );
        assert!(db.check().unwrap().is_ok());
    }
}

#[test]
fn a_damaged_page_size_is_reported_as_corruption() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    Database::open(&path).unwrap().close().unwrap();
    patch(&path, 12, &4097u32.to_le_bytes());

    let error = Database::open(&path).unwrap_err();

    assert_eq!(error.code(), "CORRUPTED");
}

#[test]
fn a_first_page_cut_short_is_reported_as_corruption() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");

    Database::open(&path).unwrap().close().unwrap();

    let whole = read(&path);

    fs::write(&path, &whole[..1024]).unwrap();

    let error = Database::open(&path).unwrap_err();

    assert_eq!(error.code(), "CORRUPTED");
}

#[test]
fn a_directory_at_the_path_is_an_io_error() {
    let dir = TestDir::new();
    let path = dir.path("folder");

    fs::create_dir(&path).unwrap();

    let error = Database::open(&path).unwrap_err();

    assert_eq!(error.code(), "IO");
}

/// Opening a database that exists writes nothing beside it, so it opens in
/// a directory that takes no new file.
#[cfg(unix)]
#[test]
fn an_existing_database_opens_in_a_directory_that_takes_no_new_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new();
    let folder = dir.path("locked");

    std::fs::create_dir(&folder).unwrap();

    let path = folder.join("app.darudb");

    Database::open(&path).unwrap().close().unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();

    // A process that may write anywhere, as root may, proves nothing here.
    let restricted = std::fs::File::create(folder.join("probe")).is_err();
    let opened = Database::open(&path).and_then(|db| {
        let mut txn = db.begin_write()?;

        txn.insert("t", b"k", b"v")?;
        txn.commit()?;
        db.close()
    });

    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();

    if restricted {
        opened.unwrap();
    }

    let names: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name != "probe")
        .collect();

    assert_eq!(names, ["app.darudb"]);
}

/// A read-only handle creates nothing, reads what other handles commit,
/// and leaves the file's bytes as they are.
#[test]
fn a_read_only_handle_reads_beside_one_that_writes() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let mut read_only = OpenOptions::new();

    read_only.read_only(true);

    // Nothing is created for it.
    assert!(matches!(read_only.open(&path), Err(Error::NotFound { .. })));
    assert!(!path.exists());

    let writer = Database::open(&path).unwrap();
    let mut txn = writer.begin_write().unwrap();

    txn.insert("t", b"k", b"1").unwrap();
    txn.commit().unwrap();

    let reader = read_only.open(&path).unwrap();

    assert!(reader.is_read_only());
    assert!(!writer.is_read_only());
    assert_eq!(
        reader.begin_read().unwrap().get("t", b"k").unwrap(),
        Some(b"1".to_vec())
    );
    assert_eq!(reader.begin_write().unwrap_err().code(), "READ_ONLY");

    // The handle beside it writes as ever, and this one reads the commit.
    let mut txn = writer.begin_write().unwrap();

    txn.insert("t", b"k", b"2").unwrap();
    txn.commit().unwrap();
    assert_eq!(
        reader.begin_read().unwrap().get("t", b"k").unwrap(),
        Some(b"2".to_vec())
    );

    writer.close().unwrap();

    let before = fs::read(&path).unwrap();

    reader.backup(dir.path("copy.darudb")).unwrap();
    reader.close().unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
}
