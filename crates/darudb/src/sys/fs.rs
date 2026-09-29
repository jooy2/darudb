//! Files: a rename that never replaces what is at the new name.
//!
//! The standard library's rename replaces a file at the new name. On
//! Unix-like systems the call goes through `rustix`, which makes it without
//! `unsafe` code here.

use std::io;
use std::path::Path;

/// Renames `from` to `to` unless something is at `to`, which fails with
/// [`io::ErrorKind::AlreadyExists`] and changes nothing: `renameat2` with
/// `RENAME_NOREPLACE` on Linux and Android, `renameatx_np` with `RENAME_EXCL`
/// on macOS and iOS, and `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING` on
/// Windows. A platform or a file system without such a rename fails with
/// [`io::ErrorKind::Unsupported`].
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    platform::rename_no_replace(from, to)
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod platform {
    use std::io;
    use std::path::Path;

    use rustix::fs::{CWD, RenameFlags, renameat_with};
    use rustix::io::Errno;

    pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
            Ok(()) => Ok(()),
            // A kernel older than the call, or a file system that does not
            // take the flag.
            Err(errno @ (Errno::NOSYS | Errno::INVAL | Errno::NOTSUP | Errno::OPNOTSUPP)) => Err(
                io::Error::new(io::ErrorKind::Unsupported, io::Error::from(errno)),
            ),
            Err(errno) => Err(errno.into()),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    /// Without `MOVEFILE_REPLACE_EXISTING`, a file at `to` fails the move with
    /// `ERROR_ALREADY_EXISTS`, which the standard library reads as
    /// [`io::ErrorKind::AlreadyExists`]. `MOVEFILE_WRITE_THROUGH` returns only
    /// once the move is on the disk, which matters on the file systems without
    /// links that take this path, since they keep no journal.
    pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let from = wide(from);
        let to = wide(to);
        // SAFETY: both names are wide strings ending in a zero, as the call
        // expects, and they live past it.
        let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };

        if moved == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    /// `path` as a wide string ending in a zero.
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    windows
)))]
mod platform {
    use std::io;
    use std::path::Path;

    pub(super) fn rename_no_replace(_from: &Path, _to: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_rename_never_replaces_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");

        fs::write(&from, b"moving").unwrap();

        match rename_no_replace(&from, &to) {
            Ok(()) => {}
            // Where the platform has none, the callers fall back.
            Err(error) if error.kind() == io::ErrorKind::Unsupported => return,
            Err(error) => panic!("{error}"),
        }

        assert_eq!(fs::read(&to).unwrap(), b"moving");
        assert!(!from.exists());

        fs::write(&from, b"second").unwrap();

        let error = rename_no_replace(&from, &to).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&to).unwrap(), b"moving");
        assert_eq!(fs::read(&from).unwrap(), b"second");
    }
}
