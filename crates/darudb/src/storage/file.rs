//! A database file opened for positional reads and writes.
//!
//! Positional I/O (`pread` and `pwrite` on Unix, `ReadFile` and `WriteFile`
//! with an offset on Windows) names the offset in every call, so the engine
//! never depends on a shared file cursor. That is what will let several
//! threads read different pages through one handle without taking a lock.

use std::fs::{self, File};
use std::io;
use std::path::Path;

/// An open database file.
#[derive(Debug)]
pub(crate) struct DbFile {
    file: File,
}

impl DbFile {
    /// Creates a new, empty file at `path` for reading and writing.
    ///
    /// Fails with [`io::ErrorKind::AlreadyExists`] if anything is already at
    /// `path`, so an existing database is never truncated.
    pub(crate) fn create_new(path: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;

        Ok(Self { file })
    }

    /// Opens the existing file at `path` for reading and writing.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new().read(true).write(true).open(path)?;

        Ok(Self { file })
    }

    /// The file itself, for the locks taken on it and for what identifies it.
    pub(crate) fn as_file(&self) -> &File {
        &self.file
    }

    /// The length of the file, in bytes.
    pub(crate) fn len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// Fills `buf` from the file, starting at `offset`.
    ///
    /// Fails with [`io::ErrorKind::UnexpectedEof`] if the file ends first.
    pub(crate) fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        platform::read_exact_at(&self.file, buf, offset)
    }

    /// Writes all of `buf` to the file, starting at `offset`.
    pub(crate) fn write_all_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        platform::write_all_at(&self.file, buf, offset)
    }

    /// Waits until everything written to the file, and its metadata, is on
    /// the storage device.
    ///
    /// On macOS and iOS this is `F_FULLFSYNC` rather than `fsync`, which the
    /// standard library chooses for us: a plain `fsync` there only reaches the
    /// drive's cache, which a power cut empties.
    pub(crate) fn sync_all(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    /// Cuts the file to `len` bytes, or extends it with zeros.
    pub(crate) fn set_len(&self, len: u64) -> io::Result<()> {
        self.file.set_len(len)
    }
}

impl super::FileIo for DbFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        self.read_exact_at(buf, offset)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        self.write_all_at(buf, offset)
    }

    fn sync(&self) -> io::Result<()> {
        self.sync_all()
    }

    fn len(&self) -> io::Result<u64> {
        DbFile::len(self)
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        DbFile::set_len(self, len)
    }
}

/// Makes the directory entry for a newly created file durable.
///
/// Creating a file changes its directory, and on Unix that change is not
/// durable until the directory itself is synced. Without this, a power cut
/// right after a database is created can leave a synced file that no directory
/// points to. Windows journals the directory change itself and offers no
/// portable way to sync a directory, so there this does nothing.
pub(crate) fn sync_parent_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };

        File::open(parent)?.sync_all()
    }

    #[cfg(windows)]
    {
        let _ = path;

        Ok(())
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::FileExt;

    pub(super) fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
        file.read_exact_at(buf, offset)
    }

    pub(super) fn write_all_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
        file.write_all_at(buf, offset)
    }
}

/// Windows has no `read_exact_at`; `seek_read` may return fewer bytes than
/// asked for, so both directions loop until the buffer is done.
#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::FileExt;

    pub(super) fn read_exact_at(
        file: &File,
        mut buf: &mut [u8],
        mut offset: u64,
    ) -> io::Result<()> {
        while !buf.is_empty() {
            match file.seek_read(buf, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the file ended before the buffer was filled",
                    ));
                }
                Ok(read) => {
                    buf = &mut buf[read..];
                    offset += read as u64;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }

    pub(super) fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match file.seek_write(buf, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "the file accepted no more bytes",
                    ));
                }
                Ok(written) => {
                    buf = &buf[written..];
                    offset += written as u64;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }
}
