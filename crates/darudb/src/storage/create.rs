//! Creating a database file so that its path never holds half of one.
//!
//! The first page is written to a temporary file beside the database and
//! synced, and only then does the file appear under its real name, through a
//! link that fails if the name is taken. A crash at any moment leaves the path
//! either empty or holding a complete file, and two processes creating the
//! same database at once cannot both succeed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::file::{DbFile, sync_parent_dir};

/// Creates the file at `path` holding exactly `contents`, unless something is
/// there already.
///
/// Returns `Ok(None)` when the path is taken, whether by an existing database
/// or by another process that won the race to create it.
pub(crate) fn create_file(path: &Path, contents: &[u8]) -> io::Result<Option<DbFile>> {
    let temporary = temporary_path(path)?;

    write_synced(&temporary, contents)?;

    let linked = fs::hard_link(&temporary, path);

    // The temporary name has done its job whether or not the link worked. A
    // leftover one is harmless: nothing ever reads it.
    let _ = fs::remove_file(&temporary);

    match linked {
        Ok(()) => {
            sync_parent_dir(path)?;

            DbFile::open(path).map(Some)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        // A file system without links, such as FAT. Create the file in place
        // instead: a crash before the contents are synced leaves an empty file,
        // which is refused as not a database rather than mistaken for one.
        Err(_) => create_in_place(path, contents),
    }
}

/// The fallback for file systems without links.
fn create_in_place(path: &Path, contents: &[u8]) -> io::Result<Option<DbFile>> {
    let file = match DbFile::create_new(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
        Err(error) => return Err(error),
    };

    file.write_all_at(contents, 0)?;
    file.sync_all()?;
    sync_parent_dir(path)?;

    Ok(Some(file))
}

/// Writes `contents` to a new file at `path` and syncs it.
fn write_synced(path: &Path, contents: &[u8]) -> io::Result<()> {
    let file = DbFile::create_new(path)?;

    file.write_all_at(contents, 0)?;
    file.sync_all()
}

/// A name beside `path` that no other process will pick: the database's own
/// name, a random suffix, and `.new`.
fn temporary_path(path: &Path) -> io::Result<PathBuf> {
    let suffix = getrandom::u64().map_err(io::Error::other)?;
    let mut name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path names no file"))?
        .to_os_string();

    name.push(format!(".{suffix:016x}.new"));

    Ok(path.with_file_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_holds_exactly_the_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.darudb");

        let file = create_file(&path, b"page zero").unwrap();

        assert!(file.is_some());
        assert_eq!(fs::read(&path).unwrap(), b"page zero");
    }

    #[test]
    fn a_taken_path_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.darudb");

        fs::write(&path, b"someone else's").unwrap();

        assert!(create_file(&path, b"page zero").unwrap().is_none());
        assert_eq!(fs::read(&path).unwrap(), b"someone else's");
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.darudb");

        create_file(&path, b"page zero").unwrap();
        create_file(&path, b"page zero").unwrap();

        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();

        assert_eq!(names, ["app.darudb"]);
    }
}
