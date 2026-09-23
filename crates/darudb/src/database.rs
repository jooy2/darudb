//! The public handle to one open database file: [`Database`].

use std::io;
use std::path::Path;

use crate::error::{Error, Result};
use crate::format::{
    CommitRecord, HEADER_LEN, HeaderError, SELECTOR_OFFSET, SLOT_COUNT, STATIC_LEN, Selector,
    StaticHeader, slot_offset,
};
use crate::options::OpenOptions;
use crate::storage::{self, DbFile};

/// An open database.
///
/// Dropping the handle closes the file. Call [`Database::close`] instead where
/// a failure to flush the file should be reported rather than ignored.
#[derive(Debug)]
pub struct Database {
    file: DbFile,
    header: StaticHeader,
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

    /// The file format version of the file, which is the one this build reads
    /// and writes: a file in any other version is refused when it is opened.
    pub fn format_version(&self) -> u32 {
        crate::FORMAT_VERSION
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
/// See [`storage::create_file`] for why the path never holds half a database.
fn create(path: &Path, page_size: u32) -> Result<Option<Database>> {
    let mut file_id = [0u8; 16];

    getrandom::fill(&mut file_id).map_err(|error| io_error(path, io::Error::other(error)))?;

    let header = StaticHeader { page_size, file_id };
    let published = CommitRecord::first();
    let page = first_page(&header, &published);

    let Some(file) = storage::create_file(path, &page).map_err(|source| io_error(path, source))?
    else {
        return Ok(None);
    };

    Ok(Some(Database { file, header }))
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
    let header_len = usize::try_from(len).map_or(HEADER_LEN, |len| len.min(HEADER_LEN));
    let mut bytes = vec![0u8; header_len];

    file.read_exact_at(&mut bytes, 0)
        .map_err(|source| io_error(path, source))?;

    let header = StaticHeader::decode(&bytes).map_err(|error| header_error(path, error))?;

    if len < u64::from(header.page_size) {
        return Err(corrupted(
            path,
            format!(
                "the file is {len} bytes long, shorter than its first page of {} bytes",
                header.page_size
            ),
        ));
    }

    let selector =
        Selector::decode(bytes[SELECTOR_OFFSET]).map_err(|reason| corrupted(path, reason))?;

    debug_assert!(selector.slot < SLOT_COUNT);

    let start = slot_offset(selector.slot);
    let published = CommitRecord::decode(selector.slot, &bytes[start..])
        .map_err(|reason| corrupted(path, format!("the published commit: {reason}")))?
        .ok_or_else(|| corrupted(path, "the selector points at an empty slot"))?;

    if published.page_count > len / u64::from(header.page_size) {
        return Err(corrupted(
            path,
            format!(
                "the published commit counts {} pages, more than the file holds",
                published.page_count
            ),
        ));
    }

    Ok(Database { file, header })
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn corrupted(path: &Path, reason: impl Into<String>) -> Error {
    Error::Corrupted {
        path: path.to_path_buf(),
        reason: reason.into(),
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
