//! DaruDB, an embedded database that keeps an application's data in one local
//! file.
//!
//! This crate is the engine and its Rust API. The Node.js and Dart packages
//! bind this same engine to their languages, so a file written from one
//! language reads the same from another.
//!
//! The engine is at an early stage. It creates a database file, writes a
//! header into it, and validates that header when the file is opened again.
//! Nothing can be stored in it yet, and the file format will change without a
//! migration until the first release.
//!
//! ```no_run
//! let db = darudb::Database::open("app.darudb")?;
//! println!("page size: {} bytes", db.page_size());
//! db.close()?;
//! # Ok::<(), darudb::Error>(())
//! ```
//!
//! # How the crate is laid out
//!
//! The modules are layered, and each one only uses the modules below it:
//!
//! - `format`: what the bytes of a file mean. Pure functions over bytes, with
//!   no I/O, so every rule about the layout can be tested without a disk.
//! - `storage`: how bytes reach the disk. Positional reads and writes, and
//!   the syncs that make them durable.
//! - [`Database`], [`OpenOptions`] and [`Error`]: the public surface, which
//!   ties the two together and reports failures with a stable code.
//!
//! The transaction, B+tree, lock, encryption, schema and query layers are
//! planned and will slot in between. `CLAUDE.md` at the repository root has
//! the whole map.

mod database;
mod error;
mod format;
mod options;
mod storage;

#[cfg(not(any(unix, windows)))]
compile_error!("DaruDB runs on Unix-like systems and Windows only.");

pub use database::Database;
pub use error::{Error, Result};
pub use format::FORMAT_VERSION;
pub use options::OpenOptions;

/// The version of this crate, as written in its `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
