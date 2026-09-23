//! The Node.js binding of DaruDB.
//!
//! This crate translates between JavaScript and the engine in `crates/darudb`
//! and decides nothing of its own: every rule about a database file lives in
//! the engine, so that Node.js and every other language read a file the same
//! way.
//!
//! Every `#[napi]` item here is part of the npm package's API. `npm run build`
//! generates `index.js` and `index.d.ts` from them, doc comments included, so
//! the comments below are what a TypeScript user reads in their editor.
//!
//! An error thrown from here is a JavaScript `Error` whose `code` is the
//! engine's [`darudb::Error::code`], unchanged.

use napi_derive::napi;

/// A result whose error becomes a JavaScript `Error` with the engine's code.
///
/// It has to be named `Result`: `#[napi]` recognises a fallible function by
/// the name of its return type, and treats any other name as a value to
/// convert.
type Result<T> = napi::Result<T, &'static str>;

/// The file format version this build of the engine reads and writes.
#[napi]
pub const FORMAT_VERSION: u32 = darudb::FORMAT_VERSION;

/// The version of the DaruDB engine inside this package.
///
/// The package's own version is in its `package.json`; the two can differ,
/// because the npm package and the engine version independently.
#[napi]
pub fn engine_version() -> &'static str {
    darudb::VERSION
}

/// Options for `Database.open`.
#[napi(object)]
pub struct OpenOptions {
    /// Whether to create the database when nothing exists at the path.
    /// Defaults to `true`. With `false`, opening a missing database throws an
    /// error whose `code` is `NOT_FOUND`. An existing file is never replaced.
    pub create: Option<bool>,
    /// The page size of a newly created database, in bytes: a power of two
    /// from 512 to 65536. Defaults to 4096. An existing database keeps the page
    /// size recorded in its file.
    pub page_size: Option<u32>,
}

/// An open DaruDB database.
///
/// Created with `Database.open`; there is no constructor. Call `close` when
/// done: the file is also closed when the object is garbage-collected, but
/// only `close` reports a failure to flush it.
#[napi]
pub struct Database {
    inner: Option<darudb::Database>,
    path: String,
}

#[napi]
impl Database {
    /// Opens the database at `path`, creating it if nothing exists there.
    #[napi(factory)]
    pub fn open(path: String, options: Option<OpenOptions>) -> Result<Self> {
        let mut open_options = darudb::OpenOptions::new();

        if let Some(options) = options {
            if let Some(create) = options.create {
                open_options.create(create);
            }

            if let Some(page_size) = options.page_size {
                open_options.page_size(page_size);
            }
        }

        let inner = open_options.open(&path).map_err(to_js_error)?;

        Ok(Self {
            inner: Some(inner),
            path,
        })
    }

    /// The path the database was opened at. Still readable after `close`.
    #[napi(getter)]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether the database is open, which is to say `close` has not been
    /// called.
    #[napi(getter)]
    pub fn is_open(&self) -> bool {
        self.inner.is_some()
    }

    /// The size of every page in the file, in bytes.
    #[napi(getter)]
    pub fn page_size(&self) -> Result<u32> {
        Ok(self.open_database()?.page_size())
    }

    /// The file format version recorded in the file.
    #[napi(getter)]
    pub fn format_version(&self) -> Result<u32> {
        Ok(self.open_database()?.format_version())
    }

    /// Flushes the file to the storage device and closes it.
    ///
    /// Closing a database that is already closed does nothing. Once this has
    /// run, reading anything but `path` and `isOpen` throws an error whose
    /// `code` is `CLOSED`.
    #[napi]
    pub fn close(&mut self) -> Result<()> {
        match self.inner.take() {
            Some(database) => database.close().map_err(to_js_error),
            None => Ok(()),
        }
    }

    fn open_database(&self) -> Result<&darudb::Database> {
        self.inner
            .as_ref()
            .ok_or_else(|| to_js_error(darudb::Error::Closed))
    }
}

/// The engine's error as a JavaScript one: the message as the message, and the
/// stable code as `code`.
fn to_js_error(error: darudb::Error) -> napi::Error<&'static str> {
    napi::Error::new(error.code(), error.to_string())
}
