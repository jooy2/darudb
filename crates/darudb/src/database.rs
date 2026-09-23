//! The public handle to one open database file: [`Database`].

use std::io;
use std::path::Path;
use std::sync::{Arc, Weak};

use crate::error::{Error, Result};
use crate::format::{
    CommitRecord, HEADER_LEN, HeaderError, SELECTOR_OFFSET, STATIC_LEN, Selector, StaticHeader,
    slot_offset,
};
use crate::instance::{FileKey, Shared, registry};
use crate::options::OpenOptions;
use crate::storage::{self, DbFile, FileIo, Pager};
use crate::txn::{ReadTransaction, WriteTransaction, recovery};

/// An open database.
///
/// Cloning a `Database`, or opening the same file again in the same process,
/// gives another handle to one shared instance: one file handle, one page
/// cache, and one writer at a time. The file is closed when the last handle
/// is dropped.
#[derive(Debug, Clone)]
pub struct Database {
    shared: Arc<Shared>,
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
        &self.shared.path
    }

    /// The size of every page in the file, in bytes.
    pub fn page_size(&self) -> u32 {
        self.shared.static_header.page_size
    }

    /// The file format version of the file, which is the one this build reads
    /// and writes: a file in any other version is refused when it is opened.
    pub fn format_version(&self) -> u32 {
        crate::FORMAT_VERSION
    }

    /// Starts a read transaction: a consistent view of the database as of the
    /// last commit.
    pub fn begin_read(&self) -> Result<ReadTransaction> {
        ReadTransaction::begin(&self.shared)
    }

    /// Starts the write transaction, waiting for one already running in
    /// another thread for up to the busy timeout.
    pub fn begin_write(&self) -> Result<WriteTransaction> {
        WriteTransaction::begin(&self.shared)
    }

    /// Closes this handle.
    ///
    /// Every commit is durable when it returns, so closing never loses data.
    /// It reports `SYNC_FAILED` if a commit through any handle to this file
    /// failed its barrier, which is the last chance to notice.
    pub fn close(self) -> Result<()> {
        self.shared.check_usable()
    }

    /// Opens or creates the database, once the options are known to be valid.
    pub(crate) fn open_with(path: &Path, options: &OpenOptions) -> Result<Self> {
        let mut instances = registry();

        instances.retain(|_, instance| instance.strong_count() > 0);

        if let Some(shared) = FileKey::of(path)
            .and_then(|key| instances.get(&key))
            .and_then(Weak::upgrade)
        {
            return Ok(Self { shared });
        }

        let created = if options.creates() {
            create(path, options.new_page_size())?
        } else {
            None
        };
        let file = match created {
            Some(file) => file,
            None => open_file(path)?,
        };
        let shared = open_io(Arc::new(file), path, options)?;

        if let Some(key) = FileKey::of(path) {
            instances.insert(key, Arc::downgrade(&shared));
        }

        Ok(Self { shared })
    }

    /// Opens a database whose file is `io`, bypassing the file system: the
    /// crash tests open their simulated disks with it.
    #[cfg(test)]
    pub(crate) fn open_io(io: Arc<dyn FileIo>, options: &OpenOptions) -> Result<Self> {
        Ok(Self {
            shared: open_io(io, Path::new("simulated.darudb"), options)?,
        })
    }

    /// Writes a new database onto the empty `io` and opens it.
    #[cfg(test)]
    pub(crate) fn create_io(io: Arc<dyn FileIo>, page_size: u32) -> Result<Self> {
        let header = StaticHeader {
            page_size,
            file_id: [7; 16],
        };
        let page = first_page(&header, &CommitRecord::first());
        let path = Path::new("simulated.darudb");

        io.write_at(&page, 0)
            .map_err(|source| io_error(path, source))?;
        io.sync().map_err(|source| io_error(path, source))?;

        Self::open_io(io, &OpenOptions::new())
    }

    /// The instance behind this handle, for the engine's own tests.
    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }
}

/// Reads the static fields of the file, runs recovery, and builds the shared
/// instance.
fn open_io(io: Arc<dyn FileIo>, path: &Path, options: &OpenOptions) -> Result<Arc<Shared>> {
    let len = io.len().map_err(|source| io_error(path, source))?;
    let header_len = usize::try_from(len).map_or(HEADER_LEN, |len| len.min(HEADER_LEN));
    let mut bytes = vec![0u8; header_len];

    io.read_at(&mut bytes, 0)
        .map_err(|source| io_error(path, source))?;

    let static_header = StaticHeader::decode(&bytes).map_err(|error| header_error(path, error))?;

    if len < u64::from(static_header.page_size) {
        return Err(Error::Corrupted {
            path: path.to_path_buf(),
            reason: format!(
                "the file is {len} bytes long, shorter than its first page of {} bytes",
                static_header.page_size
            ),
        });
    }

    let pager = Arc::new(Pager::new(
        io,
        static_header.page_size as usize,
        path.to_path_buf(),
    ));
    let shared = Shared::new(pager, path.to_path_buf(), static_header, options.busy());
    let header = recovery::recover(&shared.pager, &shared.loader)?;

    shared.set_header(header);

    Ok(Arc::new(shared))
}

/// Creates a database at `path`, or returns `None` if a file is already there.
///
/// See [`storage::create_file`] for why the path never holds half a database.
fn create(path: &Path, page_size: u32) -> Result<Option<DbFile>> {
    let mut file_id = [0u8; 16];

    getrandom::fill(&mut file_id).map_err(|error| io_error(path, io::Error::other(error)))?;

    let page = first_page(&StaticHeader { page_size, file_id }, &CommitRecord::first());

    storage::create_file(path, &page).map_err(|source| io_error(path, source))
}

/// Page 0 of a new database: the static fields, the selector pointing at
/// slot 0, and the first commit in slot 0.
fn first_page(header: &StaticHeader, first: &CommitRecord) -> Vec<u8> {
    let mut page = vec![0u8; header.page_size as usize];
    let selector = Selector {
        slot: 0,
        unsynced: false,
    };

    page[..STATIC_LEN].copy_from_slice(&header.encode());
    page[SELECTOR_OFFSET] = selector.encode();
    page[slot_offset(0)..slot_offset(1)].copy_from_slice(&first.encode(0));

    page
}

/// Opens the file already at `path`.
fn open_file(path: &Path) -> Result<DbFile> {
    DbFile::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Error::NotFound {
            path: path.to_path_buf(),
        },
        _ => io_error(path, error),
    })
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
        HeaderError::Damaged(reason) => Error::Corrupted {
            path,
            reason: reason.to_owned(),
        },
    }
}
