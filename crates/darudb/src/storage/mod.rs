//! How bytes reach the disk.
//!
//! The layers above ask for bytes at an offset and for a sync; this module is
//! where those requests meet the operating system. The page cache, page
//! checks and page encryption will live here as well, so that nothing above
//! this layer ever sees a page that has not been verified.
//!
//! The file is never mapped into memory. Another process can change a mapped
//! file under a live reference, which safe Rust cannot express, and mapping
//! conflicts with both encryption and sharing a file between processes.
//! Positional reads and writes through a page cache of our own avoid all three.

mod create;
mod file;

pub(crate) use create::create_file;
pub(crate) use file::DbFile;
