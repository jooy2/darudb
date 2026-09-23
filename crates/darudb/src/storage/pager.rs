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

/// The most bytes one read or write of consecutive pages moves at once: few
/// enough calls into the operating system for a long run, and a bounded
/// buffer for a large value.
const RUN_BYTES: usize = 1 << 20;

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

    /// How many consecutive pages one read or write moves at most.
    pub(crate) fn run_pages(&self) -> usize {
        (RUN_BYTES / self.page_size).max(1)
    }

    /// Reads page `page` and verifies it against `expected`.
    pub(crate) fn read(&self, page: u64, expected: &Check) -> Result<Vec<u8>> {
        let bytes = self.read_unverified(page)?;

        if page_check(page, &bytes) != *expected {
            return Err(self.corrupted(format!("page {page} fails its check")));
        }

        Ok(bytes)
    }

    /// Reads `count` consecutive pages from `first` on, in one call, and
    /// verifies each against the check stored in the page itself. Returns the
    /// bytes and each page's check. For overflow pages, whose parent keeps one
    /// check for the whole run.
    pub(crate) fn read_run_self_checked(
        &self,
        first: u64,
        count: usize,
    ) -> Result<(Vec<u8>, Vec<Check>)> {
        let mut bytes = vec![0u8; count * self.page_size];

        self.read_into(first, &mut bytes)?;

        let mut checks = Vec::with_capacity(count);

        for (page, content) in (first..).zip(bytes.chunks(self.page_size)) {
            let check = stored_check(content);

            if page_check(page, content) != check {
                return Err(self.corrupted(format!("page {page} fails its check")));
            }

            checks.push(check);
        }

        Ok((bytes, checks))
    }

    fn read_unverified(&self, page: u64) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; self.page_size];

        self.read_into(page, &mut bytes)?;

        Ok(bytes)
    }

    /// Fills `bytes` with the pages from `first` on.
    fn read_into(&self, first: u64, bytes: &mut [u8]) -> Result<()> {
        match self.io.read_at(bytes, self.offset(first)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(self.corrupted(
                format!("a read from page {first} on runs past the end of the file"),
            )),
            Err(source) => Err(self.io_error(source)),
        }
    }

    /// Seals each page of `bytes`, consecutive pages from `first` on, with its
    /// check, and writes them in one call. Returns the checks, which the
    /// pages' parents record.
    pub(crate) fn write_run(&self, first: u64, bytes: &mut [u8]) -> Result<Vec<Check>> {
        debug_assert_eq!(bytes.len() % self.page_size, 0);

        let checks = (first..)
            .zip(bytes.chunks_mut(self.page_size))
            .map(|(page, content)| seal(page, content))
            .collect();

        self.io
            .write_at(bytes, self.offset(first)?)
            .map_err(|source| self.io_error(source))?;

        Ok(checks)
    }

    /// Writes consecutive pages from `first` on, each already sealed with its
    /// check, in one call.
    pub(crate) fn write_sealed_run(&self, first: u64, bytes: &[u8]) -> Result<()> {
        debug_assert_eq!(bytes.len() % self.page_size, 0);
        debug_assert!(
            (first..)
                .zip(bytes.chunks(self.page_size))
                .all(|(page, content)| page_check(page, content) == stored_check(content))
        );

        self.io
            .write_at(bytes, self.offset(first)?)
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
