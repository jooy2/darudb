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

    /// Writes all of `bufs`, one after another, to the file, starting at
    /// `offset`.
    pub(crate) fn write_all_vectored_at(&self, bufs: &[&[u8]], offset: u64) -> io::Result<()> {
        platform::write_all_vectored_at(&self.file, bufs, offset)
    }

    /// Waits until everything written to the file, and the metadata needed
    /// to read it back, such as its length, is on the storage device.
    ///
    /// On Linux, Android and the BSDs this is `fdatasync`, which leaves out
    /// the file's times. `fsync` journals them too, which made a sync commit
    /// a third slower on an ext4 file system and gains a commit nothing,
    /// since nothing reads them. On macOS and iOS it is `F_FULLFSYNC` rather
    /// than `fsync`, which the standard library chooses for us: a plain
    /// `fsync` there only reaches the drive's cache, which a power cut
    /// empties. On Windows it is `FlushFileBuffers`.
    pub(crate) fn sync_data(&self) -> io::Result<()> {
        self.file.sync_data()
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

    fn write_vectored_at(&self, bufs: &[&[u8]], offset: u64) -> io::Result<()> {
        self.write_all_vectored_at(bufs, offset)
    }

    fn sync(&self) -> io::Result<()> {
        self.sync_data()
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

    /// `pwritev`, called again for what a call leaves unwritten. Joining the
    /// buffers and writing them with one `pwrite` took about a tenth of a
    /// commit of many pages, in the copy. Where the system lacks the call,
    /// as macOS did before version 11, the rest is joined and written so.
    pub(super) fn write_all_vectored_at(
        file: &File,
        bufs: &[&[u8]],
        mut offset: u64,
    ) -> io::Result<()> {
        let mut slices: Vec<io::IoSlice<'_>> =
            bufs.iter().map(|buf| io::IoSlice::new(buf)).collect();
        let mut left = &mut slices[..];

        while !left.is_empty() {
            match rustix::io::pwritev(file, left, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "the file accepted no more bytes",
                    ));
                }
                Ok(written) => {
                    offset += written as u64;
                    io::IoSlice::advance_slices(&mut left, written);
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::NOSYS) => {
                    let rest: Vec<&[u8]> = left.iter().map(|slice| &**slice).collect();

                    return file.write_all_at(&rest.concat(), offset);
                }
                Err(error) => return Err(error.into()),
            }
        }

        Ok(())
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

    /// Windows writes a gathered set of buffers only to a file opened
    /// without buffering, so the buffers are joined and written as one.
    pub(super) fn write_all_vectored_at(
        file: &File,
        bufs: &[&[u8]],
        offset: u64,
    ) -> io::Result<()> {
        write_all_at(file, &bufs.concat(), offset)
    }
}

#[cfg(test)]
mod tests {
    use super::DbFile;
    use crate::testing::Rng;

    /// Buffers written where they lie land one after another as they would
    /// joined, whatever their sizes and however many there are, past the
    /// end of the file too, and a later write changes only the bytes it
    /// names.
    #[test]
    fn buffers_written_where_they_lie_land_as_they_would_joined() {
        let dir = tempfile::tempdir().unwrap();
        let file = DbFile::create_new(&dir.path().join("vectored")).unwrap();
        let mut rng = Rng::new(3);
        let mut expected = Vec::new();

        for round in 0..200 {
            let count = rng.index(2000);
            let bufs: Vec<Vec<u8>> = (0..count)
                .map(|_| {
                    let len = [0, 1, 4096, rng.index(9000)][rng.index(4)];

                    rng.bytes(len)
                })
                .collect();
            let joined = bufs.concat();
            let offset = rng.index(expected.len() + 5000);
            let slices: Vec<&[u8]> = bufs.iter().map(Vec::as_slice).collect();

            file.write_all_vectored_at(&slices, offset as u64).unwrap();

            if expected.len() < offset + joined.len() {
                expected.resize(offset + joined.len(), 0);
            }

            expected[offset..offset + joined.len()].copy_from_slice(&joined);

            let mut read = vec![0; expected.len()];

            file.read_exact_at(&mut read, 0).unwrap();
            assert!(read == expected, "round {round}");
        }
    }
}
