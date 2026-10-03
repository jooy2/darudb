//! Files: a rename that never replaces what is at the new name, and on
//! Windows what identifies a file.
//!
//! The standard library's rename replaces a file at the new name, and its
//! file identity on Windows is not stable yet. On Unix-like systems both calls
//! go through `rustix`, which makes them without `unsafe` code here.

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

/// What identifies the open file `file` on Windows: its volume's serial
/// number and its file index, the same through every path to it.
#[cfg(windows)]
pub(crate) fn identity(file: &std::fs::File) -> io::Result<(u64, u64)> {
    platform::identity(file)
}

/// What identifies the file at `path` on Windows; see [`identity`]. It opens
/// a handle that reads nothing but the file's attributes, which is harmless
/// beside the handle of a database: on Windows a lock belongs to its handle,
/// so closing this one releases none.
#[cfg(windows)]
pub(crate) fn identity_at(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;

    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .open(path)?;

    identity(&file)
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
            // take the flag. `ENOTSUP` and `EOPNOTSUPP` are one number on
            // Linux and two on Apple's systems, so they are compared rather
            // than matched.
            Err(errno)
                if [Errno::NOSYS, Errno::INVAL, Errno::NOTSUP, Errno::OPNOTSUPP]
                    .contains(&errno) =>
            {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    io::Error::from(errno),
                ))
            }
            Err(errno) => Err(errno.into()),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

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

    pub(super) fn identity(file: &File) -> io::Result<(u64, u64)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is the open file's, valid for the call, and the
        // call fills `info`, which lives past it.
        let found = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) };

        if found == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
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

    #[cfg(windows)]
    #[test]
    fn two_paths_to_one_file_have_one_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        let link = dir.path().join("link");

        fs::write(&path, b"one").unwrap();
        fs::write(dir.path().join("other"), b"two").unwrap();
        fs::hard_link(&path, &link).unwrap();

        let found = identity_at(&path).unwrap();

        assert_eq!(identity_at(&link).unwrap(), found);
        assert_eq!(identity(&fs::File::open(&path).unwrap()).unwrap(), found);
        assert_ne!(identity_at(&dir.path().join("other")).unwrap(), found);
    }
}
