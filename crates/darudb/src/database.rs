//! The public handle to one open database file: [`Database`].

use std::io;
use std::path::Path;

use crate::error::{Error, Result};
use crate::format::{FileHeader, HEADER_LEN, HeaderError};
use crate::options::OpenOptions;
use crate::storage::{self, DbFile};

/// An open database.
///
/// Dropping the handle closes the file. Call [`Database::close`] instead where
/// a failure to flush the file should be reported rather than ignored.
#[derive(Debug)]
pub struct Database {
    file: DbFile,
    header: FileHeader,
}

impl Database {
    /// Opens the database at `path`, creating it if nothing exists there.
    ///
    /// The same as [`OpenOptions::new`] followed by [`OpenOptions::open`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        OpenOptions::new().open(path)
    }

    /// The path the database was opened at.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// The size of every page in the file, in bytes.
    pub fn page_size(&self) -> u32 {
        self.header.page_size
    }

    /// The file format version recorded in the file.
    pub fn format_version(&self) -> u32 {
        self.header.format_version
    }

    /// Flushes the file to the storage device and closes it.
    pub fn close(self) -> Result<()> {
        self.file.sync_all().map_err(|source| Error::Io {
            path: self.file.path().to_path_buf(),
            source,
        })
    }

    /// Opens or creates the database, once the options are known to be valid.
    pub(crate) fn open_with(path: &Path, options: &OpenOptions) -> Result<Self> {
        if options.creates() {
            if let Some(database) = create(path, options.new_page_size())? {
                return Ok(database);
            }
        }

        open_existing(path)
    }
}

/// Creates a database at `path`, or returns `None` if a file is already there.
///
/// The file is created with `create_new`, so an existing file is never
/// truncated, and page 0 is synced before anything else can rely on it.
///
/// Two processes creating the same database at once can still collide: one
/// may open the file between the other's creating it and writing its header,
/// find it empty, and be refused with `NOT_A_DATABASE`. The file is not damaged
/// and a retry succeeds. The lock protocol closes this by writing the header
/// under the writer lock.
fn create(path: &Path, page_size: u32) -> Result<Option<Database>> {
    let file = match DbFile::create_new(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
        Err(source) => return Err(io_error(path, source)),
    };

    let header = FileHeader::new(page_size);

    if let Err(source) = write_first_page(&file, &header) {
        // Leave nothing behind that a later open would refuse as damaged, so
        // that the caller can simply try again.
        drop(file);
        let _ = std::fs::remove_file(path);

        return Err(io_error(path, source));
    }

    storage::sync_parent_dir(path).map_err(|source| io_error(path, source))?;

    Ok(Some(Database { file, header }))
}

/// Writes page 0, the header followed by zeros, and syncs it.
fn write_first_page(file: &DbFile, header: &FileHeader) -> io::Result<()> {
    let mut page = vec![0u8; header.page_size as usize];

    page[..HEADER_LEN].copy_from_slice(&header.encode());
    file.write_all_at(&page, 0)?;
    file.sync_all()
}

/// Opens the database file already at `path` and validates its header.
fn open_existing(path: &Path) -> Result<Database> {
    let file = match DbFile::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(Error::NotFound {
                path: path.to_path_buf(),
            });
        }
        Err(source) => return Err(io_error(path, source)),
    };

    let len = file.len().map_err(|source| io_error(path, source))?;

    if len < HEADER_LEN as u64 {
        return Err(Error::NotADatabase {
            path: path.to_path_buf(),
        });
    }

    let mut bytes = [0u8; HEADER_LEN];

    file.read_exact_at(&mut bytes, 0)
        .map_err(|source| io_error(path, source))?;

    let header = FileHeader::decode(&bytes).map_err(|error| header_error(path, error))?;

    if len < u64::from(header.page_size) {
        return Err(Error::Corrupted {
            path: path.to_path_buf(),
            reason: format!(
                "the file is {len} bytes long, shorter than its first page of {} bytes",
                header.page_size
            ),
        });
    }

    Ok(Database { file, header })
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn header_error(path: &Path, error: HeaderError) -> Error {
    let path = path.to_path_buf();

    match error {
        HeaderError::NotADatabase => Error::NotADatabase { path },
        HeaderError::UnsupportedVersion(found) => Error::UnsupportedFormatVersion {
            path,
            found,
            supported: crate::FORMAT_VERSION,
        },
        HeaderError::InvalidPageSize(size) => Error::Corrupted {
            path,
            reason: format!("the header records a page size of {size} bytes"),
        },
    }
}
