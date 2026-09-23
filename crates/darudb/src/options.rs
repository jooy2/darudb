//! How a database is opened: [`OpenOptions`].

use std::path::Path;
use std::time::Duration;

use crate::database::Database;
use crate::error::{Error, Result};
use crate::format::{self, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, MIN_PAGE_SIZE};
use crate::instance::Settings;

/// Options for opening a database, in the style of [`std::fs::OpenOptions`].
///
/// ```no_run
/// use darudb::OpenOptions;
///
/// let db = OpenOptions::new().create(false).open("app.darudb")?;
/// # Ok::<(), darudb::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct OpenOptions {
    create: bool,
    page_size: u32,
    busy_timeout: Duration,
    max_unsynced_pages: u64,
    max_unsynced_time: Duration,
}

impl OpenOptions {
    /// The defaults: create the database if it does not exist, with the
    /// default page size.
    pub fn new() -> Self {
        Self {
            create: true,
            page_size: DEFAULT_PAGE_SIZE,
            busy_timeout: Duration::from_secs(5),
            max_unsynced_pages: 16_384,
            max_unsynced_time: Duration::from_secs(1),
        }
    }

    /// Whether to create the database when nothing exists at the path.
    ///
    /// On by default. With it off, opening a path where nothing exists fails
    /// with [`Error::NotFound`]. An existing file is never replaced either way.
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    /// The page size of a newly created database, in bytes.
    ///
    /// A power of two from 4096 to 65536. It only applies when the database is
    /// created: an existing file keeps the page size recorded in its header.
    /// The default, 4096, is provisional until the benchmarks settle it.
    pub fn page_size(&mut self, bytes: u32) -> &mut Self {
        self.page_size = bytes;
        self
    }

    /// How long [`Database::begin_write`] waits for a write transaction that
    /// is already running before failing with [`Error::Busy`]. Five seconds by
    /// default.
    ///
    /// Every handle to a file in one process shares one instance, and the
    /// options of the handle that opened the file first apply to all of them.
    pub fn busy_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.busy_timeout = timeout;
        self
    }

    /// How many pages deferred commits may write before one of them is made
    /// durable anyway. 16384 by default: 64 MiB with 4096-byte pages.
    ///
    /// The limit bounds what a power cut can undo, and how much recovery has
    /// to check after one. The default is provisional until the benchmarks
    /// settle it.
    pub fn max_unsynced_pages(&mut self, pages: u64) -> &mut Self {
        self.max_unsynced_pages = pages;
        self
    }

    /// How long deferred commits may go without a barrier. One second by
    /// default.
    ///
    /// When the time is up, a thread the engine starts for the purpose makes
    /// them durable, as [`Database::sync`] would. If a write transaction holds
    /// the writer lock at that moment, the thread waits for it, and a deferred
    /// commit made after the time is up is made durable itself. The thread
    /// exists only while deferred commits are waiting.
    pub fn max_unsynced_time(&mut self, time: Duration) -> &mut Self {
        self.max_unsynced_time = time;
        self
    }

    /// Opens the database at `path` with these options.
    pub fn open(&self, path: impl AsRef<Path>) -> Result<Database> {
        self.validate()?;

        Database::open_with(path.as_ref(), self)
    }

    pub(crate) fn creates(&self) -> bool {
        self.create
    }

    pub(crate) fn new_page_size(&self) -> u32 {
        self.page_size
    }

    pub(crate) fn settings(&self) -> Settings {
        Settings {
            busy_timeout: self.busy_timeout,
            max_unsynced_pages: self.max_unsynced_pages,
            max_unsynced_time: self.max_unsynced_time,
        }
    }

    fn validate(&self) -> Result<()> {
        if !format::is_valid_page_size(self.page_size) {
            return Err(Error::InvalidArgument {
                message: format!(
                    "the page size must be a power of two from {MIN_PAGE_SIZE} to {MAX_PAGE_SIZE}, not {}",
                    self.page_size
                ),
            });
        }

        Ok(())
    }
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}
