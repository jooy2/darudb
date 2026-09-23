//! Reading and writing whole pages, each verified against the check it is
//! expected to have.
//!
//! Nothing above this layer ever sees a page that has not been verified. A
//! page is read only through a pointer, which carries the page's check, and a
//! page whose bytes do not produce that check is reported as damaged.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::FileIo;
use crate::error::{Error, Result};
use crate::format::{Check, page_check, seal, stored_check};

/// Page-sized reads and writes on one database file.
#[derive(Debug)]
pub(crate) struct Pager {
    io: Arc<dyn FileIo>,
    page_size: usize,
    path: PathBuf,
}

impl Pager {
    pub(crate) fn new(io: Arc<dyn FileIo>, page_size: usize, path: PathBuf) -> Self {
        Self {
            io,
            page_size,
            path,
        }
    }

    /// The size of every page, in bytes.
    pub(crate) fn page_size(&self) -> usize {
        self.page_size
    }

    /// The path of the file, for error messages.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Reads page `page` and verifies it against `expected`.
    pub(crate) fn read(&self, page: u64, expected: &Check) -> Result<Vec<u8>> {
        let bytes = self.read_unverified(page)?;

        if page_check(page, &bytes) != *expected {
            return Err(self.corrupted(format!("page {page} fails its check")));
        }

        Ok(bytes)
    }

    /// Reads page `page` and verifies it against the check stored in the page
    /// itself, which it returns. For overflow pages, whose parent keeps one
    /// check for the whole run.
    pub(crate) fn read_self_checked(&self, page: u64) -> Result<(Vec<u8>, Check)> {
        let bytes = self.read_unverified(page)?;
        let check = stored_check(&bytes);

        if page_check(page, &bytes) != check {
            return Err(self.corrupted(format!("page {page} fails its check")));
        }

        Ok((bytes, check))
    }

    fn read_unverified(&self, page: u64) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; self.page_size];
        let offset = self.offset(page)?;

        match self.io.read_at(&mut bytes, offset) {
            Ok(()) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                Err(self.corrupted(format!("page {page} lies past the end of the file")))
            }
            Err(source) => Err(self.io_error(source)),
        }
    }

    /// Seals `bytes` with its check and writes it as page `page`. Returns the
    /// check, which the page's parent records.
    pub(crate) fn write(&self, page: u64, bytes: &mut [u8]) -> Result<Check> {
        debug_assert_eq!(bytes.len(), self.page_size);

        let check = seal(page, bytes);

        self.io
            .write_at(bytes, self.offset(page)?)
            .map_err(|source| self.io_error(source))?;

        Ok(check)
    }

    /// Writes page `page`, already sealed with its check.
    pub(crate) fn write_sealed(&self, page: u64, bytes: &[u8]) -> Result<()> {
        debug_assert_eq!(bytes.len(), self.page_size);
        debug_assert_eq!(page_check(page, bytes), stored_check(bytes));

        self.io
            .write_at(bytes, self.offset(page)?)
            .map_err(|source| self.io_error(source))
    }

    /// Writes `bytes` at `offset` of page 0, for the selector and the slots.
    pub(crate) fn write_header(&self, bytes: &[u8], offset: usize) -> Result<()> {
        self.io
            .write_at(bytes, offset as u64)
            .map_err(|source| self.io_error(source))
    }

    /// Reads the first `len` bytes of the file.
    pub(crate) fn read_header(&self, len: usize) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; len];

        self.io
            .read_at(&mut bytes, 0)
            .map_err(|source| self.io_error(source))?;

        Ok(bytes)
    }

    /// A barrier: everything written before it is durable when it returns.
    pub(crate) fn sync(&self) -> io::Result<()> {
        self.io.sync()
    }

    /// The length of the file, in bytes.
    pub(crate) fn file_len(&self) -> Result<u64> {
        self.io.len().map_err(|source| self.io_error(source))
    }

    /// Makes the file exactly `pages` pages long: cuts it, or extends it with
    /// zeros.
    pub(crate) fn resize(&self, pages: u64) -> Result<()> {
        let len = pages
            .checked_mul(self.page_size as u64)
            .ok_or_else(|| self.corrupted(format!("{pages} pages exceed any file")))?;

        self.io.set_len(len).map_err(|source| self.io_error(source))
    }

    fn offset(&self, page: u64) -> Result<u64> {
        page.checked_mul(self.page_size as u64)
            .ok_or_else(|| self.corrupted(format!("page {page} lies past any file")))
    }

    pub(crate) fn corrupted(&self, reason: String) -> Error {
        Error::Corrupted {
            path: self.path.clone(),
            reason,
        }
    }

    pub(crate) fn io_error(&self, source: io::Error) -> Error {
        Error::Io {
            path: self.path.clone(),
            source,
        }
    }
}
