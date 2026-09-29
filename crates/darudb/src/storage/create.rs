//! Creating a database file so that its path never holds half of one.
//!
//! The first page is written to a temporary file beside the database and
//! synced, and only then does the file appear under its real name, through a
//! link that fails if the name is taken. A crash at any moment leaves the path
//! either empty or holding a complete file, and two processes creating the
//! same database at once cannot both succeed.
//!
//! A file system without links, such as FAT, gets the temporary file renamed
//! to the real name with the platform's rename that never replaces a file,
//! which keeps the same promises. One without that either gets the file
//! created empty in place, and the caller writes the first page into it with
//! [`fill`] while holding the open lock exclusively, so that no other process
//! reads it half-written. A crash in between leaves an empty file, which is
//! refused as not a database.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::file::{DbFile, sync_parent_dir};

/// A file [`create_file`] made.
#[derive(Debug)]
pub(crate) enum Created {
    /// The file appeared whole, holding the contents.
    Whole(DbFile),
    /// The file system has no links: the file is empty, and [`fill`] writes
    /// the contents into it.
    Empty(DbFile),
}

/// Creates the file at `path` holding exactly `contents`, unless something is
/// there already, or empty where the file system has no links.
///
/// Returns `Ok(None)` when the path is taken, whether by an existing database
/// or by another process that won the race to create it.
pub(crate) fn create_file(path: &Path, contents: &[u8]) -> io::Result<Option<Created>> {
    let temporary = temporary_path(path)?;

    write_synced(&temporary, contents)?;

    let placed = place(&temporary, path);

    // The temporary name has done its job whether or not it took the path,
    // and after a rename it is gone already. A leftover one is harmless:
    // nothing ever reads it.
    let _ = fs::remove_file(&temporary);

    match placed {
        Ok(true) => {
            sync_parent_dir(path)?;

            DbFile::open(path).map(|file| Some(Created::Whole(file)))
        }
        // Neither links nor a no-replace rename. Create the file in place
        // instead, for the caller to fill.
        Ok(false) => match DbFile::create_new(path) {
            Ok(file) => Ok(Some(Created::Empty(file))),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(error),
        },
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes `contents` into the empty file at `path` that [`create_file`]
/// created in place, and makes it durable.
pub(crate) fn fill(file: &DbFile, path: &Path, contents: &[u8]) -> io::Result<()> {
    file.write_all_at(contents, 0)?;
    file.sync_all()?;
    sync_parent_dir(path)
}

/// Writes `contents` to a new file beside `path`, under a name no other
/// process will pick, syncs it, and returns its name and the open file, for
/// a caller that fills the file before it moves it to `path` with
/// [`move_into_place`].
pub(crate) fn create_beside(path: &Path, contents: &[u8]) -> io::Result<(PathBuf, DbFile)> {
    let temporary = temporary_path(path)?;

    write_synced(&temporary, contents)?;

    let file = DbFile::open(&temporary)?;

    Ok((temporary, file))
}

/// Gives the file at `temporary` the name `path`, unless something is there
/// already, which fails with [`io::ErrorKind::AlreadyExists`] and leaves it
/// alone. The temporary name goes either way. A file system with neither
/// links nor a no-replace rename gets a copy under the new name, made only if
/// nothing is there.
pub(crate) fn move_into_place(temporary: &Path, path: &Path) -> io::Result<()> {
    let moved = match place(temporary, path) {
        Ok(true) => Ok(()),
        Ok(false) => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut target| {
                io::copy(&mut fs::File::open(temporary)?, &mut target)?;
                target.sync_all()
            }),
        Err(error) => Err(error),
    };
    let _ = fs::remove_file(temporary);

    moved?;
    sync_parent_dir(path)
}

/// Gives the file at `temporary` the name `path` without replacing anything
/// there: through a link, which leaves the temporary name for the caller to
/// remove, or where the file system has no links, a rename that never
/// replaces a file. A taken path fails with [`io::ErrorKind::AlreadyExists`].
/// Returns whether the file took the name: `false` when the file system has
/// neither.
fn place(temporary: &Path, path: &Path) -> io::Result<bool> {
    match link(temporary, path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        Err(_) => match rename(temporary, path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
            Err(_) => Ok(false),
        },
    }
}

/// [`fs::hard_link`], which the tests can make fail as on a file system
/// without links.
fn link(original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(test)]
    if crate::testing::NO_LINKS.get() {
        return Err(io::ErrorKind::Unsupported.into());
    }

    fs::hard_link(original, link)
}

/// [`rename_no_replace`](crate::sys::fs::rename_no_replace), which the tests
/// can take away too.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(test)]
    if crate::testing::NO_RENAME.get() {
        return Err(io::ErrorKind::Unsupported.into());
    }

    crate::sys::fs::rename_no_replace(from, to)
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

        assert!(matches!(file, Some(Created::Whole(_))));
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

    /// Every way a file takes its name, as the file systems that have links,
    /// only a no-replace rename, or neither, give it.
    #[test]
    fn a_file_takes_its_name_without_links_and_without_a_rename() {
        use crate::testing::{NO_LINKS, NO_RENAME};

        for (links, rename) in [(true, true), (false, true), (false, false)] {
            // A platform without the rename is covered by the last case.
            if !links
                && rename
                && cfg!(not(any(
                    target_os = "linux",
                    target_os = "android",
                    target_vendor = "apple",
                    windows
                )))
            {
                continue;
            }

            NO_LINKS.set(!links);
            NO_RENAME.set(!rename);

            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("app.darudb");
            let created = create_file(&path, b"page zero").unwrap().unwrap();

            match (created, links || rename) {
                (Created::Whole(_), true) => assert_eq!(fs::read(&path).unwrap(), b"page zero"),
                (Created::Empty(file), false) => fill(&file, &path, b"page zero").unwrap(),
                (_, named) => panic!("links {links}, rename {rename}: named {named}"),
            }

            assert!(create_file(&path, b"another").unwrap().is_none());
            assert_eq!(fs::read(&path).unwrap(), b"page zero");

            // The tools' way: a file written beside the path, then moved.
            let target = dir.path().join("copy.darudb");
            let (temporary, _) = create_beside(&target, b"a copy").unwrap();

            move_into_place(&temporary, &target).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"a copy");

            let (temporary, _) = create_beside(&target, b"no copy").unwrap();

            assert_eq!(
                move_into_place(&temporary, &target).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
            assert_eq!(fs::read(&target).unwrap(), b"a copy");

            let mut names: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();

            names.sort();
            assert_eq!(
                names,
                ["app.darudb", "copy.darudb"],
                "links {links}, rename {rename}"
            );
        }

        NO_LINKS.set(false);
        NO_RENAME.set(false);
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
