//! DaruDB, an embedded database that keeps an application's data in one local
//! file.
//!
//! This crate is the engine and its Rust API. The Node.js and Dart packages
//! bind this same engine to their languages, so a file written from one
//! language reads the same from another.
//!
//! The storage kernel stores named trees of byte keys and byte values, in
//! transactions. A commit is durable when it returns, and a file opened after
//! a crash or a power cut holds the last commit that returned. Typed records
//! and queries come later, built on top. The file format will change without
//! a migration until the first release, and only one process may have a file
//! open at a time for now.
//!
//! ```no_run
//! let db = darudb::Database::open("app.darudb")?;
//! let mut txn = db.begin_write()?;
//!
//! txn.insert("users", b"alice", b"admin")?;
//! txn.commit()?;
//!
//! let read = db.begin_read()?;
//!
//! assert_eq!(read.get("users", b"alice")?, Some(b"admin".to_vec()));
//! # Ok::<(), darudb::Error>(())
//! ```
//!
//! # How the crate is laid out
//!
//! The modules are layered, and each one only uses the modules below it:
//!
//! - `format`: what the bytes of a file mean. Pure functions over bytes, with
//!   no I/O, so every rule about the layout can be tested without a disk.
//! - `storage`: how bytes reach the disk: positional reads and writes, pages
//!   verified against their checks, the page cache.
//! - `btree`: copy-on-write B+trees over those pages.
//! - `space`: which pages a write transaction may use, and which it gives back.
//! - `instance` and `txn`: the shared state of an open file, transactions, the
//!   commit and recovery.
//! - [`Database`], [`OpenOptions`] and [`Error`]: the public surface.
//!
//! `design/` at the repository root specifies the file format and the commit
//! protocol this crate implements, and `CLAUDE.md` has the whole map.

mod btree;
mod database;
mod error;
mod format;
mod instance;
mod options;
mod space;
mod storage;
#[cfg(test)]
mod testing;
mod txn;

#[cfg(not(any(unix, windows)))]
compile_error!("DaruDB runs on Unix-like systems and Windows only.");

pub use database::Database;
pub use error::{Error, Result};
pub use format::FORMAT_VERSION;
pub use options::OpenOptions;
pub use txn::{Range, ReadTransaction, WriteTransaction};

/// The version of this crate, as written in its `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
