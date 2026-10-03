//! The native half of the Dart binding of DaruDB: the engine in
//! `crates/darudb` behind a C interface, which `lib/src/native.dart` calls
//! through `dart:ffi`.
//!
//! It translates and decides nothing of its own, as the Node.js binding's
//! does not: objects cross as records and queries as IR, the byte formats of
//! `design/objects.md`, which the Dart side writes and reads, so that a batch
//! of objects costs one call and one buffer.
//!
//! The interface keeps to a few rules, so that each function reads alike:
//!
//! - **Handles** are pointers this library made: a database, a transaction,
//!   a prepared query, a collection's name. Each has a function that frees
//!   it, which the Dart side calls once, from its finalizer or when the
//!   handle is closed.
//! - **Status.** A function that can fail returns an `i32`: `-1` for a
//!   failure, whose code and message [`darudb_last_error`] gives, and
//!   otherwise `0`, or `1` where it answers a yes or no question.
//! - **Bytes in** are a pointer and a length, read during the call only.
//! - **Bytes out** go into a buffer this thread keeps, which a [`Buf`] points
//!   into until the next call on the thread that returns bytes. A Dart
//!   isolate runs on one thread between its awaits, so it reads them before
//!   anything else can write there.
//! - **Panics** never cross into Dart: each function catches one and reports
//!   it as `INTERNAL`.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use zeroize::Zeroizing;

mod record;

use record::Reader;

/// Bytes this library hands to Dart: where they are and how many.
#[repr(C)]
pub struct Buf {
    /// The first byte.
    pub ptr: *const u8,
    /// How many bytes.
    pub len: usize,
}

/// What a database tells about itself.
#[repr(C)]
pub struct Info {
    /// The size of every page in the file, in bytes.
    pub page_size: u32,
    /// The file format version recorded in the file.
    pub format_version: u32,
    /// 1 if the file is encrypted, and 0 otherwise.
    pub encrypted: u8,
}

/// A failure as Dart gets it: the engine's error code, unchanged, and its
/// message.
struct Failure {
    code: &'static str,
    message: String,
}

impl From<darudb::Error> for Failure {
    fn from(error: darudb::Error) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

type Result<T> = std::result::Result<T, Failure>;

fn invalid(message: impl Into<String>) -> Failure {
    Failure {
        code: "INVALID_ARGUMENT",
        message: message.into(),
    }
}

fn closed() -> Failure {
    darudb::Error::Closed.into()
}

thread_local! {
    /// The last failure on this thread, for [`darudb_last_error`].
    static LAST_ERROR: RefCell<Failure> = const {
        RefCell::new(Failure { code: "", message: String::new() })
    };

    /// The bytes the last call on this thread handed out.
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Runs `body`, turning a failure or a panic into `-1` and keeping it for
/// [`darudb_last_error`].
fn guard(body: impl FnOnce() -> Result<i32>) -> i32 {
    let failure = match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(status)) => return status,
        Ok(Err(failure)) => failure,
        Err(panic) => Failure {
            code: "INTERNAL",
            message: panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "the engine panicked".to_owned()),
        },
    };

    LAST_ERROR.with_borrow_mut(|last| *last = failure);

    -1
}

/// Hands out bytes that `fill` writes into this thread's buffer, through
/// `out`.
///
/// # Safety
///
/// `out` points to a `Buf` the caller can write.
unsafe fn hand_out<T>(out: *mut Buf, fill: impl FnOnce(&mut Vec<u8>) -> Result<T>) -> Result<T> {
    OUT.with_borrow_mut(|bytes| {
        bytes.clear();

        let value = fill(bytes)?;

        // SAFETY: the caller promises `out` is writable; the buffer lives in
        // this thread until its next use, which is after Dart has read it.
        unsafe {
            out.write(Buf {
                ptr: bytes.as_ptr(),
                len: bytes.len(),
            });
        }

        // A large result does not keep its memory for every later call.
        if bytes.capacity() > 1 << 20 && bytes.len() < 1 << 16 {
            bytes.shrink_to(1 << 16);
        }

        Ok(value)
    })
}

/// The `len` bytes at `ptr`, or none when `len` is 0.
///
/// # Safety
///
/// `ptr` points to `len` readable bytes that do not change during the call.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 || ptr.is_null() {
        return &[];
    }

    // SAFETY: the caller promises `len` readable bytes at `ptr`.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// The UTF-8 text in the `len` bytes at `ptr`.
///
/// # Safety
///
/// As [`bytes`].
unsafe fn text<'a>(ptr: *const u8, len: usize) -> Result<&'a str> {
    // SAFETY: the caller's promise, passed on.
    let bytes = unsafe { bytes(ptr, len) };

    std::str::from_utf8(bytes).map_err(|_| invalid("a name or a path that is not UTF-8"))
}

/// Writes `value` through `out`, if the caller gave somewhere to.
///
/// # Safety
///
/// `out` is null or points to a `T` the caller can write.
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        // SAFETY: the caller promises `out` is writable when it is not null.
        unsafe { out.write(value) };
    }
}

/// Gives the code and the message of the last failure on this thread.
///
/// # Safety
///
/// `code` and `message` point to `Buf`s the caller can write. They point at
/// memory of this library until the next failure on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_last_error(code: *mut Buf, message: *mut Buf) {
    LAST_ERROR.with_borrow(|last| {
        // SAFETY: the caller promises both are writable or null; the code is
        // static and the message lives until the next failure.
        unsafe {
            put(
                code,
                Buf {
                    ptr: last.code.as_ptr(),
                    len: last.code.len(),
                },
            );
        }
        // SAFETY: as above.
        unsafe {
            put(
                message,
                Buf {
                    ptr: last.message.as_ptr(),
                    len: last.message.len(),
                },
            );
        }
    });
}

/// The version of the engine, as its crate names it.
///
/// # Safety
///
/// `out` points to a `Buf` the caller can write; it points at static memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_engine_version(out: *mut Buf) {
    // SAFETY: the caller promises `out` is writable.
    unsafe {
        put(
            out,
            Buf {
                ptr: darudb::VERSION.as_ptr(),
                len: darudb::VERSION.len(),
            },
        );
    }
}

/// The file format version this build of the engine reads and writes.
#[unsafe(no_mangle)]
pub extern "C" fn darudb_format_version() -> u32 {
    darudb::FORMAT_VERSION
}

/// An open database. A read lock serves every call; closing takes the write
/// lock, so a call never finds the database gone halfway through.
pub struct Database {
    inner: RwLock<Option<darudb::Database>>,
}

impl Database {
    fn boxed(database: darudb::Database) -> *mut Self {
        Box::into_raw(Box::new(Self {
            inner: RwLock::new(Some(database)),
        }))
    }

    fn with<T>(&self, operation: impl FnOnce(&darudb::Database) -> Result<T>) -> Result<T> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);

        operation(inner.as_ref().ok_or_else(closed)?)
    }
}

/// What a transaction handle holds: a read or a write transaction, or a
/// migration under way, whose write transaction its functions use. `None`
/// once it has committed or ended.
enum Txn {
    Read(Box<darudb::ReadTransaction>),
    Write(Box<darudb::WriteTransaction>),
    Migration(Box<darudb::PendingMigration>),
}

/// A transaction handle. A mutex, so that the asynchronous API can run a
/// transaction's work on another thread; the Dart side sends it one call at
/// a time, so the mutex is never contended.
pub struct Held(Mutex<Option<Txn>>);

fn handle_of(txn: Txn) -> *const Held {
    Arc::into_raw(Arc::new(Held(Mutex::new(Some(txn)))))
}

fn lock(held: &Held) -> MutexGuard<'_, Option<Txn>> {
    held.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `operation` on the transaction `held` holds.
fn with_txn<T>(held: &Held, operation: impl FnOnce(&mut Txn) -> Result<T>) -> Result<T> {
    let mut txn = lock(held);

    operation(txn.as_mut().ok_or_else(ended)?)
}

fn ended() -> Failure {
    invalid("the transaction has ended")
}

impl Txn {
    fn writing(&mut self) -> Result<&mut darudb::WriteTransaction> {
        match self {
            Txn::Read(_) => Err(invalid("a read transaction does not write")),
            Txn::Write(txn) => Ok(txn),
            Txn::Migration(pending) => Ok(pending.transaction()),
        }
    }

    fn migration(&mut self) -> Result<&mut darudb::PendingMigration> {
        match self {
            Txn::Migration(pending) => Ok(pending),
            _ => Err(invalid("the transaction is not a migration")),
        }
    }

    /// Runs `read` on collection `name` of a read or a write transaction.
    fn reading<T>(
        &mut self,
        name: &str,
        read: impl FnOnce(Collection<'_>) -> darudb::Result<T>,
    ) -> Result<T> {
        match self {
            Txn::Read(txn) => txn.collection(name).and_then(|c| read(Collection::Read(c))),
            _ => self
                .writing()?
                .collection(name)
                .and_then(|c| read(Collection::Write(c))),
        }
        .map_err(Failure::from)
    }
}

/// A collection a transaction reads, of either kind.
enum Collection<'a> {
    Read(darudb::CollectionReader<'a>),
    Write(darudb::CollectionWriter<'a>),
}

impl Collection<'_> {
    fn get_record_with(
        &self,
        key: darudb::Value,
        visit: impl FnMut(&[u8]) -> darudb::Result<()>,
    ) -> darudb::Result<bool> {
        match self {
            Collection::Read(c) => c.get_record_with(key, visit),
            Collection::Write(c) => c.get_record_with(key, visit),
        }
    }

    fn query_records_with(
        &self,
        query: &darudb::Query,
        visit: impl FnMut(&[u8]) -> darudb::Result<()>,
    ) -> darudb::Result<()> {
        match self {
            Collection::Read(c) => c.query_records_with(query, visit),
            Collection::Write(c) => c.query_records_with(query, visit),
        }
    }

    fn count(&self, query: &darudb::Query) -> darudb::Result<u64> {
        match self {
            Collection::Read(c) => c.count(query),
            Collection::Write(c) => c.count(query),
        }
    }
}

/// The options of `Database.open`, from their record:
///
/// | Field | Value                                                      |
/// | ----- | ---------------------------------------------------------- |
/// | 1     | `bool`: create the file when it does not exist             |
/// | 2     | `int`: the page size of a new file                         |
/// | 3     | `int`: the busy timeout, in milliseconds                   |
/// | 4     | `int`: the page cache's size, in bytes                     |
/// | 5     | `bytes`: the declared schema, as `Schema::decode` reads it |
/// | 6     | `bytes`: a key of 32 bytes                                 |
/// | 7     | `bytes`: a password                                        |
/// | 8     | `object`: password hashing, fields 1 to 3 the memory in    |
/// |       | KiB, the iterations and the lanes                          |
/// | 9     | `list(object)`: migrations, as [`migration_of`] reads them |
fn open_options(record: &[u8]) -> Result<darudb::OpenOptions> {
    let mut options = darudb::OpenOptions::new();

    for field in Reader::new(record).fields()? {
        let (id, value) = field?;

        match id {
            1 => {
                options.create(value.bool()?);
            }
            2 => {
                options.page_size(
                    u32::try_from(value.int()?)
                        .map_err(|_| invalid("a page size beyond 32 bits"))?,
                );
            }
            3 => {
                let milliseconds =
                    u64::try_from(value.int()?).map_err(|_| invalid("a negative busy timeout"))?;

                options.busy_timeout(std::time::Duration::from_millis(milliseconds));
            }
            4 => {
                options.cache_size(usize::try_from(value.int()?).unwrap_or(usize::MAX));
            }
            5 => {
                options.schema(darudb::Schema::decode(value.bytes()?)?);
            }
            6 => {
                let key = Zeroizing::new(
                    <[u8; 32]>::try_from(value.bytes()?)
                        .map_err(|_| invalid("a key is 32 bytes long"))?,
                );

                options.key(*key);
            }
            7 => {
                options.password(value.bytes()?);
            }
            8 => {
                let mut cost = [0u32; 3];

                for field in value.object()?.fields()? {
                    let (id, value) = field?;
                    let slot = usize::try_from(id)
                        .ok()
                        .and_then(|id| id.checked_sub(1))
                        .and_then(|at| cost.get_mut(at))
                        .ok_or_else(|| invalid("password hashing has three fields"))?;

                    *slot = u32::try_from(value.int()?)
                        .map_err(|_| invalid("a password hashing cost beyond 32 bits"))?;
                }

                options.password_hashing(cost[0], cost[1], cost[2]);
            }
            9 => {
                for migration in value.list()? {
                    options.migration(migration_of(migration?.object()?)?);
                }
            }
            _ => {
                return Err(invalid(format!(
                    "an option the library does not know, {id}"
                )));
            }
        }
    }

    Ok(options)
}

/// A migration step's changes besides its function, from their record:
/// field 1 the version, 2 the collections renamed as objects of the old name
/// (1) and the new (2), 3 the fields renamed as objects of the collection
/// (1), the old name (2) and the new (3), 4 the names of the collections
/// deleted, and 5 the fields replaced as objects of the collection (1) and
/// the field (2).
fn migration_of(record: Reader<'_>) -> Result<darudb::Migration> {
    let mut migration = None;
    let mut changes = Vec::new();

    for field in record.fields()? {
        let (id, value) = field?;

        match id {
            1 => {
                migration = Some(darudb::Migration::to(
                    u64::try_from(value.int()?)
                        .map_err(|_| invalid("a negative schema version"))?,
                ));
            }
            2..=5 => changes.push((id, value)),
            _ => {
                return Err(invalid(format!(
                    "a migration field the library does not know, {id}"
                )));
            }
        }
    }

    let mut migration = migration.ok_or_else(|| invalid("a migration without a version"))?;

    for (id, value) in changes {
        for item in value.list()? {
            let item = item?;

            migration = match id {
                2 => {
                    let [from, to] = item.object()?.strings::<2>()?;

                    migration.rename_collection(from, to)
                }
                3 => {
                    let [collection, from, to] = item.object()?.strings::<3>()?;

                    migration.rename_field(collection, from, to)
                }
                4 => migration.delete_collection(item.string()?),
                _ => {
                    let [collection, field] = item.object()?.strings::<2>()?;

                    migration.replace_field(collection, field)
                }
            };
        }
    }

    Ok(migration)
}

/// Opens the database at `path` with the options in the record at
/// `options`. Returns 0 with the database in `database`, or 1 with the
/// migration under way in `migration`, when the file held an older schema.
///
/// # Safety
///
/// `path` and `options` point to as many readable bytes as their lengths
/// say; `database` and `migration` to handles the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_open(
    path: *const u8,
    path_len: usize,
    options: *const u8,
    options_len: usize,
    database: *mut *mut Database,
    migration: *mut *const Held,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `path`.
        let path = unsafe { text(path, path_len) }?;
        // SAFETY: the caller's promise for `options`.
        let options = unsafe { bytes(options, options_len) };

        match open_options(options)?.open_migrating(path)? {
            darudb::Opening::Open(opened) => {
                // SAFETY: the caller's promise for `database`.
                unsafe { put(database, Database::boxed(opened)) };

                Ok(0)
            }
            darudb::Opening::Migrating(pending) => {
                // SAFETY: the caller's promise for `migration`.
                unsafe { put(migration, handle_of(Txn::Migration(Box::new(pending)))) };

                Ok(1)
            }
        }
    })
}

/// Frees a database handle, closing the database first if it is open. Its
/// transactions live on.
///
/// # Safety
///
/// `database` came from this library and is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_database_free(database: *mut Database) {
    if !database.is_null() {
        // SAFETY: the caller hands the box back, once.
        drop(unsafe { Box::from_raw(database) });
    }
}

/// The database a handle points to.
///
/// # Safety
///
/// `database` came from this library and has not been freed.
unsafe fn database_at<'a>(database: *const Database) -> Result<&'a Database> {
    // SAFETY: the caller's promise; null is refused.
    unsafe { database.as_ref() }.ok_or_else(closed)
}

/// Closes the database, making deferred commits durable first. The handle
/// stays valid until it is freed, and every later call on it fails with
/// `CLOSED`. Closing a closed database does nothing.
///
/// # Safety
///
/// `database` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_close(database: *const Database) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;
        let inner = database
            .inner
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        match inner {
            Some(inner) => inner.close().map(|()| 0).map_err(Failure::from),
            None => Ok(0),
        }
    })
}

/// Fills `info` with the page size, the format version and whether the file
/// is encrypted.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `info` points
/// to an `Info` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_database_info(database: *const Database, info: *mut Info) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;
        let found = database.with(|database| {
            Ok(Info {
                page_size: database.page_size(),
                format_version: database.format_version(),
                encrypted: u8::from(database.is_encrypted()),
            })
        })?;

        // SAFETY: the caller's promise for `info`.
        unsafe { put(info, found) };

        Ok(0)
    })
}

/// The stored schema the database was opened with, as its record. Returns 1
/// with it in `out`, or 0 for a database opened without a schema.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `out` points
/// to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_schema_record(database: *const Database, out: *mut Buf) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;

        database.with(|database| {
            // SAFETY: the caller's promise for `out`.
            unsafe {
                hand_out(out, |bytes| {
                    Ok(match database.schema_record() {
                        Some(record) => {
                            bytes.extend_from_slice(record);
                            1
                        }
                        None => 0,
                    })
                })
            }
        })
    })
}

/// Begins a read transaction, into `txn`.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `txn` points
/// to a handle the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_begin_read(
    database: *const Database,
    txn: *mut *const Held,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;
        let read = database.with(|database| database.begin_read().map_err(Failure::from))?;

        // SAFETY: the caller's promise for `txn`.
        unsafe { put(txn, handle_of(Txn::Read(Box::new(read)))) };

        Ok(0)
    })
}

/// Begins a write transaction, into `txn`, waiting for another writer up to
/// the busy timeout.
///
/// # Safety
///
/// As [`darudb_begin_read`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_begin_write(
    database: *const Database,
    txn: *mut *const Held,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;
        let write = database.with(|database| database.begin_write().map_err(Failure::from))?;

        // SAFETY: the caller's promise for `txn`.
        unsafe { put(txn, handle_of(Txn::Write(Box::new(write)))) };

        Ok(0)
    })
}

/// Makes every deferred commit durable.
///
/// # Safety
///
/// `database` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_sync(database: *const Database) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;

        database.with(|database| database.sync().map(|()| 0).map_err(Failure::from))
    })
}

/// Changes the key of an encrypted database to the 32 bytes at `key`.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `key` points to
/// `key_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_set_key(
    database: *const Database,
    key: *const u8,
    key_len: usize,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;
        // SAFETY: the caller's promise for `key`.
        let key = Zeroizing::new(
            <[u8; 32]>::try_from(unsafe { bytes(key, key_len) })
                .map_err(|_| invalid("a key is 32 bytes long"))?,
        );

        database.with(|database| database.set_key(*key).map(|()| 0).map_err(Failure::from))
    })
}

/// Changes the password of an encrypted database to the bytes at
/// `password`.
///
/// # Safety
///
/// As [`darudb_set_key`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_set_password(
    database: *const Database,
    password: *const u8,
    password_len: usize,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let database = unsafe { database_at(database) }?;
        // SAFETY: the caller's promise for `password`.
        let password = unsafe { bytes(password, password_len) };

        database.with(|database| {
            database
                .set_password(password)
                .map(|()| 0)
                .map_err(Failure::from)
        })
    })
}

/// The transaction mutex a handle points to.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
unsafe fn held_at<'a>(txn: *const Held) -> Result<&'a Held> {
    // SAFETY: the caller's promise; null is refused.
    unsafe { txn.as_ref() }.ok_or_else(ended)
}

/// Frees a transaction handle, ending the transaction if it has not ended:
/// a write's changes are thrown away.
///
/// # Safety
///
/// `txn` came from this library and is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_txn_free(txn: *const Held) {
    if !txn.is_null() {
        // SAFETY: the caller hands the reference back, once.
        drop(unsafe { Arc::from_raw(txn) });
    }
}

/// Ends the transaction, throwing a write's changes away, and keeps the
/// handle until it is freed. Ending an ended one does nothing.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_txn_end(txn: *const Held) {
    // SAFETY: the caller's promise.
    if let Ok(held) = unsafe { held_at(txn) } {
        *lock(held) = None;
    }
}

/// Commits a write transaction, deferred or not, and ends it.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_commit(txn: *const Held, deferred: u8) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let held = unsafe { held_at(txn) }?;
        let taken = lock(held).take().ok_or_else(ended)?;

        match taken {
            Txn::Write(txn) if deferred != 0 => txn.commit_deferred(),
            Txn::Write(txn) => txn.commit(),
            _ => return Err(invalid("only a write transaction commits")),
        }
        .map(|()| 0)
        .map_err(Failure::from)
    })
}

/// The name of a collection, made once for the calls that read and write
/// its objects rather than read from bytes on every call.
///
/// # Safety
///
/// `name` points to `name_len` readable bytes. The result is freed with
/// [`darudb_name_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_name(name: *const u8, name_len: usize) -> *mut String {
    // SAFETY: the caller's promise.
    let name = unsafe { bytes(name, name_len) };

    Box::into_raw(Box::new(String::from_utf8_lossy(name).into_owned()))
}

/// Frees a collection's name.
///
/// # Safety
///
/// `name` came from [`darudb_name`] and is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_name_free(name: *mut String) {
    if !name.is_null() {
        // SAFETY: the caller hands the box back, once.
        drop(unsafe { Box::from_raw(name) });
    }
}

/// The name a handle points to.
///
/// # Safety
///
/// `name` came from [`darudb_name`] and has not been freed.
unsafe fn name_at<'a>(name: *const String) -> Result<&'a str> {
    // SAFETY: the caller's promise; null is refused.
    unsafe { name.as_ref() }
        .map(String::as_str)
        .ok_or_else(|| invalid("a collection without a name"))
}

/// A primary key, from the value at `key`: a record's `int`, `string` or
/// `bytes`, tag first.
///
/// # Safety
///
/// `key` points to `key_len` readable bytes.
unsafe fn key_at(key: *const u8, key_len: usize) -> Result<darudb::Value> {
    // SAFETY: the caller's promise.
    let bytes = unsafe { bytes(key, key_len) };

    Reader::new(bytes).value()?.key()
}

/// The record of the object of collection `name` whose primary key is the
/// value at `key`. Returns 1 with it in `out`, or 0 when there is none.
///
/// # Safety
///
/// `txn` and `name` came from this library and have not been freed; `key`
/// points to `key_len` readable bytes; `out` to a `Buf` the caller can
/// write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_get(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name)? };
        // SAFETY: the caller's promise for `key`.
        let key = unsafe { key_at(key, key_len)? };

        with_txn(held, |txn| {
            // SAFETY: the caller's promise for `out`.
            unsafe {
                hand_out(out, |bytes| {
                    txn.reading(name, |collection| {
                        collection.get_record_with(key, |record| {
                            bytes.extend_from_slice(record);

                            Ok(())
                        })
                    })
                    .map(i32::from)
                })
            }
        })
    })
}

/// Appends `value` as a varint, as a record's lengths are written.
fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value.to_le_bytes()[0] | 0x80);
        value >>= 7;
    }

    out.push(value.to_le_bytes()[0]);
}

/// Hands out the records `query` finds on collection `name`, each after its
/// length.
///
/// # Safety
///
/// `out` points to a `Buf` the caller can write.
unsafe fn find_records(
    txn: &mut Txn,
    name: &str,
    query: &darudb::Query,
    out: *mut Buf,
) -> Result<i32> {
    // SAFETY: the caller's promise for `out`.
    unsafe {
        hand_out(out, |bytes| {
            txn.reading(name, |collection| {
                collection.query_records_with(query, |record| {
                    push_varint(bytes, record.len() as u64);
                    bytes.extend_from_slice(record);

                    Ok(())
                })
            })
            .map(|()| 0)
        })
    }
}

/// The query whose IR is at `ir`, the first object alone with `first`.
///
/// # Safety
///
/// `ir` points to `ir_len` readable bytes.
unsafe fn request_at(ir: *const u8, ir_len: usize, first: u8) -> Result<darudb::QueryRequest> {
    // SAFETY: the caller's promise.
    let mut request = darudb::QueryRequest::decode(unsafe { bytes(ir, ir_len) })?;

    if first != 0 {
        request.query = request.query.first();
    }

    Ok(request)
}

/// The records of the objects the query whose IR is at `ir` finds, one after
/// another, each after its length; only the first with `first`.
///
/// # Safety
///
/// `txn` came from this library and has not been freed; `ir` points to
/// `ir_len` readable bytes; `out` to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_find(
    txn: *const Held,
    ir: *const u8,
    ir_len: usize,
    first: u8,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `ir`.
        let request = unsafe { request_at(ir, ir_len, first)? };

        // SAFETY: the caller's promise for `out`.
        with_txn(held, |txn| unsafe {
            find_records(txn, &request.collection, &request.query, out)
        })
    })
}

/// How many objects the query whose IR is at `ir` finds, into `count`.
///
/// # Safety
///
/// `txn` came from this library and has not been freed; `ir` points to
/// `ir_len` readable bytes; `count` to a `u64` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_count(
    txn: *const Held,
    ir: *const u8,
    ir_len: usize,
    count: *mut u64,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `ir`.
        let request = unsafe { request_at(ir, ir_len, 0)? };
        let found = with_txn(held, |txn| {
            txn.reading(&request.collection, |collection| {
                collection.count(&request.query)
            })
        })?;

        // SAFETY: the caller's promise for `count`.
        unsafe { put(count, found) };

        Ok(0)
    })
}

/// A query parsed once, for a collection, which each run gives its
/// parameters' values.
pub struct Prepared {
    collection: String,
    query: darudb::Query,
}

/// Prepares the query in the query language at `text` on collection `name`,
/// into `prepared`.
///
/// # Safety
///
/// `name` and `text` point to as many readable bytes as their lengths say;
/// `prepared` to a handle the caller can write. The result is freed with
/// [`darudb_prepared_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_prepare_text(
    name: *const u8,
    name_len: usize,
    query: *const u8,
    query_len: usize,
    prepared: *mut *const Prepared,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { text(name, name_len)? };
        // SAFETY: the caller's promise for `query`.
        let query = unsafe { text(query, query_len)? };
        let made = Prepared {
            collection: name.to_owned(),
            query: darudb::Query::prepare(query)?,
        };

        // SAFETY: the caller's promise for `prepared`.
        unsafe { put(prepared, Arc::into_raw(Arc::new(made))) };

        Ok(0)
    })
}

/// Prepares the query whose IR is at `ir`, which holds parameters in place
/// of values, into `prepared`.
///
/// # Safety
///
/// As [`darudb_prepare_text`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_prepare_ir(
    ir: *const u8,
    ir_len: usize,
    prepared: *mut *const Prepared,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let request = unsafe { request_at(ir, ir_len, 0) }?;
        let made = Prepared {
            collection: request.collection,
            query: request.query,
        };

        // SAFETY: the caller's promise for `prepared`.
        unsafe { put(prepared, Arc::into_raw(Arc::new(made))) };

        Ok(0)
    })
}

/// Frees a prepared query.
///
/// # Safety
///
/// `prepared` came from this library and is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_prepared_free(prepared: *const Prepared) {
    if !prepared.is_null() {
        // SAFETY: the caller hands the reference back, once.
        drop(unsafe { Arc::from_raw(prepared) });
    }
}

/// The prepared query, bound to the parameters' values in the record at
/// `parameters` (`design/objects.md`, "The IR"), the first alone with
/// `first`, and the collection it is on.
///
/// # Safety
///
/// `prepared` came from this library and has not been freed; `parameters`
/// points to `parameters_len` readable bytes.
unsafe fn bound<'a>(
    prepared: *const Prepared,
    parameters: *const u8,
    parameters_len: usize,
    first: u8,
) -> Result<(&'a str, darudb::Query)> {
    // SAFETY: the caller's promise; null is refused.
    let prepared =
        unsafe { prepared.as_ref() }.ok_or_else(|| invalid("a prepared query that is gone"))?;
    // SAFETY: the caller's promise for `parameters`.
    let mut query = prepared
        .query
        .bind_encoded(unsafe { bytes(parameters, parameters_len) })?;

    if first != 0 {
        query = query.first();
    }

    Ok((&prepared.collection, query))
}

/// The records a prepared query finds with the parameters' values at
/// `parameters`, as [`darudb_find`] gives them.
///
/// # Safety
///
/// `txn` and `prepared` came from this library and have not been freed;
/// `parameters` points to `parameters_len` readable bytes; `out` to a `Buf`
/// the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_find_prepared(
    txn: *const Held,
    prepared: *const Prepared,
    parameters: *const u8,
    parameters_len: usize,
    first: u8,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `prepared` and `parameters`.
        let (name, query) = unsafe { bound(prepared, parameters, parameters_len, first)? };

        // SAFETY: the caller's promise for `out`.
        with_txn(held, |txn| unsafe { find_records(txn, name, &query, out) })
    })
}

/// How many objects a prepared query finds, into `count`.
///
/// # Safety
///
/// As [`darudb_find_prepared`], with `count` a `u64` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_count_prepared(
    txn: *const Held,
    prepared: *const Prepared,
    parameters: *const u8,
    parameters_len: usize,
    count: *mut u64,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `prepared` and `parameters`.
        let (name, query) = unsafe { bound(prepared, parameters, parameters_len, 0)? };
        let found = with_txn(held, |txn| {
            txn.reading(name, |collection| collection.count(&query))
        })?;

        // SAFETY: the caller's promise for `count`.
        unsafe { put(count, found) };

        Ok(0)
    })
}

/// Writes a key as a record's value, tag first.
fn write_key(out: &mut Vec<u8>, key: &darudb::Value) -> Result<()> {
    match key {
        darudb::Value::Int(value) => {
            out.push(record::INT);
            push_varint(out, record::zigzag(*value));
        }
        darudb::Value::String(value) => {
            out.push(record::STRING);
            push_varint(out, value.len() as u64);
            out.extend_from_slice(value.as_bytes());
        }
        darudb::Value::Bytes(value) => {
            out.push(record::BYTES);
            push_varint(out, value.len() as u64);
            out.extend_from_slice(value);
        }
        _ => return Err(invalid("a primary key of another type")),
    }

    Ok(())
}

/// Inserts, or with `replace` puts, the objects whose records are at
/// `records`, each after its length, in collection `name`, and hands out
/// their keys, each a record's value. A refused record stops the batch with
/// its error, and the records before it stay written.
///
/// # Safety
///
/// `txn` and `name` came from this library and have not been freed;
/// `records` points to `records_len` readable bytes; `out` to a `Buf` the
/// caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_write(
    txn: *const Held,
    name: *const String,
    records: *const u8,
    records_len: usize,
    replace: u8,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name)? };
        // SAFETY: the caller's promise for `records`.
        let records = unsafe { bytes(records, records_len) };

        with_txn(held, |txn| {
            let mut writer = txn.writing()?.collection(name)?;

            // SAFETY: the caller's promise for `out`.
            unsafe {
                hand_out(out, |keys| {
                    let mut reader = Reader::new(records);

                    while !reader.is_empty() {
                        let record = reader.counted()?;
                        let key = if replace != 0 {
                            writer.put_record(record)
                        } else {
                            writer.insert_record(record)
                        }?;

                        write_key(keys, &key)?;
                    }

                    Ok(0)
                })
            }
        })
    })
}

/// Sets the fields the record of changes at `changes` holds in the object
/// of collection `name` whose primary key is the value at `key`. Returns 1
/// if there was one, and 0 otherwise.
///
/// # Safety
///
/// `txn` and `name` came from this library and have not been freed; `key`
/// and `changes` point to as many readable bytes as their lengths say.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_update(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    changes: *const u8,
    changes_len: usize,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name)? };
        // SAFETY: the caller's promise for `key`.
        let key = unsafe { key_at(key, key_len)? };
        // SAFETY: the caller's promise for `changes`.
        let changes = unsafe { bytes(changes, changes_len) };

        with_txn(held, |txn| {
            let found = txn
                .writing()?
                .collection(name)?
                .update_record(key, changes)?;

            Ok(i32::from(found))
        })
    })
}

/// Deletes the object of collection `name` whose primary key is the value at
/// `key`. Returns 1 if there was one, and 0 otherwise.
///
/// # Safety
///
/// `txn` and `name` came from this library and have not been freed; `key`
/// points to `key_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_delete(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name)? };
        // SAFETY: the caller's promise for `key`.
        let key = unsafe { key_at(key, key_len)? };

        with_txn(held, |txn| {
            let found = txn.writing()?.collection(name)?.delete(key)?;

            Ok(i32::from(found))
        })
    })
}

/// The schema version the file held before the migration, into `version`.
///
/// # Safety
///
/// `txn` came from this library and has not been freed; `version` points to
/// a `u64` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_previous_version(
    txn: *const Held,
    version: *mut u64,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let held = unsafe { held_at(txn) }?;
        let found = with_txn(held, |txn| Ok(txn.migration()?.previous_version()))?;

        // SAFETY: the caller's promise for `version`.
        unsafe { put(version, found) };

        Ok(0)
    })
}

/// The record of the schema the migration leads to, with `previous` 0, or
/// of the one the file held before it, with `previous` 1.
///
/// # Safety
///
/// `txn` came from this library and has not been freed; `out` points to a
/// `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_schema_record(
    txn: *const Held,
    previous: u8,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let held = unsafe { held_at(txn) }?;

        with_txn(held, |txn| {
            let migration = txn.migration()?;

            // SAFETY: the caller's promise for `out`.
            unsafe {
                hand_out(out, |bytes| {
                    if previous != 0 {
                        bytes.extend_from_slice(&migration.previous_schema_record());
                    } else {
                        bytes.extend_from_slice(migration.schema_record());
                    }

                    Ok(0)
                })
            }
        })
    })
}

/// Runs the engine's part of the next version step, and returns 1 with the
/// step's version in `version`, or 0 once every step has run. The Dart side
/// runs the step's own function after each.
///
/// # Safety
///
/// `txn` came from this library and has not been freed; `version` points to
/// a `u64` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_next_step(txn: *const Held, version: *mut u64) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let held = unsafe { held_at(txn) }?;
        let step = with_txn(held, |txn| {
            txn.migration()?.next_step().map_err(Failure::from)
        })?;

        match step {
            Some(step) => {
                // SAFETY: the caller's promise for `version`.
                unsafe { put(version, step) };

                Ok(1)
            }
            None => Ok(0),
        }
    })
}

/// The record of the object of collection `name`, as the schema before the
/// migration named it, whose primary key is the value at `key`, as that
/// schema reads it. Returns 1 with it in `out`, or 0 when there is none.
///
/// # Safety
///
/// As [`darudb_get`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_previous_record(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name)? };
        // SAFETY: the caller's promise for `key`.
        let key = unsafe { key_at(key, key_len)? };

        with_txn(held, |txn| {
            let record = txn.migration()?.migrating().previous_record(name, key)?;

            // SAFETY: the caller's promise for `out`.
            unsafe {
                hand_out(out, |bytes| {
                    Ok(match record {
                        Some(record) => {
                            bytes.extend_from_slice(&record);
                            1
                        }
                        None => 0,
                    })
                })
            }
        })
    })
}

/// The primary keys of every object of collection `name`, as the schema
/// before the migration named it, in key order: each a record's value.
///
/// # Safety
///
/// `txn` and `name` came from this library and have not been freed; `out`
/// points to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_previous_keys(
    txn: *const Held,
    name: *const String,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { held_at(txn)? };
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name)? };

        with_txn(held, |txn| {
            let keys = txn.migration()?.migrating().previous_keys(name)?;

            // SAFETY: the caller's promise for `out`.
            unsafe {
                hand_out(out, |bytes| {
                    for key in &keys {
                        write_key(bytes, key)?;
                    }

                    Ok(0)
                })
            }
        })
    })
}

/// Runs the steps left, deletes the collections the steps delete, commits
/// the migration, and gives the open database in `database`. The
/// transaction ends.
///
/// # Safety
///
/// `txn` came from this library and has not been freed; `database` points
/// to a handle the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_finish(
    txn: *const Held,
    database: *mut *mut Database,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise.
        let held = unsafe { held_at(txn) }?;
        let taken = lock(held).take().ok_or_else(ended)?;
        let Txn::Migration(pending) = taken else {
            return Err(invalid("only a migration finishes"));
        };
        let opened = pending.finish()?;

        // SAFETY: the caller's promise for `database`.
        unsafe { put(database, Database::boxed(opened)) };

        Ok(0)
    })
}
