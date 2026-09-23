//! How bytes reach the disk.
//!
//! The layers above ask for pages and for barriers; this module is where those
//! requests meet the operating system. Every page is verified against its
//! expected check here, so nothing above this layer ever sees one that has not
//! been. Page encryption will live here as well.
//!
//! The file is never mapped into memory. Another process can change a mapped
//! file under a live reference, which safe Rust cannot express, and mapping
//! conflicts with both encryption and sharing a file between processes.
//! Positional reads and writes through a page cache of our own avoid all three.

mod cache;
mod create;
mod file;
mod pager;
#[cfg(test)]
pub(crate) mod sim;

use std::fmt::Debug;
use std::io;

pub(crate) use cache::Cache;
pub(crate) use create::create_file;
pub(crate) use file::DbFile;
pub(crate) use pager::Pager;

/// Positional I/O on one file. The real file implements it, and so does the
/// simulated disk the crash tests use in its place.
pub(crate) trait FileIo: Send + Sync + Debug {
    /// Fills `buf` from `offset`, failing with
    /// [`io::ErrorKind::UnexpectedEof`] if the file ends first.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;

    /// Writes all of `buf` at `offset`, extending the file if needed.
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()>;

    /// A barrier: when it returns successfully, every earlier write is
    /// durable.
    fn sync(&self) -> io::Result<()>;

    /// The length of the file, in bytes.
    fn len(&self) -> io::Result<u64>;

    /// Cuts the file to `len` bytes, or extends it with zeros.
    fn set_len(&self, len: u64) -> io::Result<()>;
}
