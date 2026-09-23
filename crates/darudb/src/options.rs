//! How a database is opened: [`OpenOptions`].

use std::path::Path;

use crate::database::Database;
use crate::error::{Error, Result};
use crate::format::{self, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, MIN_PAGE_SIZE};

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
}

impl OpenOptions {
    /// The defaults: create the database if it does not exist, with the
    /// default page size.
    pub fn new() -> Self {
        Self {
            create: true,
            page_size: DEFAULT_PAGE_SIZE,
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
