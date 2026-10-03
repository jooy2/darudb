//! The native half of the Dart binding of DaruDB: the engine in
//! `crates/darudb` behind a C interface, which `lib/src/native.dart` calls
//! through `dart:ffi`.
//!
//! It translates and decides nothing of its own, as the Node.js binding's
//! does not: objects cross as records and queries as IR, the byte formats of
//! `design/objects.md`, which the Dart side writes and reads, so that a batch
//! of objects costs one call and one buffer.
//!
//! - `ops`: what each call does, once for both kinds of call.
//! - `work`: the asynchronous calls, which run on threads of this library
//!   and hand their result to a callback.
//! - `record`: the few records from Dart this library reads itself.
//!
//! The interface keeps to a few rules, so that each function reads alike:
//!
//! - **Handles** are pointers this library made: a database, a transaction,
//!   a prepared query, a collection's name. Each has a function that frees
//!   it, which the Dart side calls once, from its finalizer or when the
//!   handle is closed. An asynchronous call holds a reference of its own to
//!   the handles it uses, so freeing one while a call runs is safe.
//! - **Status.** A function that can fail returns an `i32`: `-1` for a
//!   failure, whose code and message [`darudb_last_error`] gives, and
//!   otherwise `0`, or `1` where it answers a yes or no question.
//! - **Bytes in** are a pointer and a length, read during the call only.
//! - **Bytes out** of a synchronous call go into a buffer this thread keeps,
//!   which a [`Buf`] points into until the next call on the thread that
//!   returns bytes. A Dart isolate runs on one thread between its awaits, so
//!   it reads them before anything else can write there.
//! - **Panics** never cross into Dart: each function catches one and reports
//!   it as `INTERNAL`.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

mod ops;
mod record;
mod work;

pub use ops::{Database, Held, Prepared};

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
pub(crate) struct Failure {
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

pub(crate) type Result<T> = std::result::Result<T, Failure>;

pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    Failure {
        code: "INVALID_ARGUMENT",
        message: message.into(),
    }
}

pub(crate) fn closed() -> Failure {
    darudb::Error::Closed.into()
}

thread_local! {
    /// The last failure on this thread, for [`darudb_last_error`].
    static LAST_ERROR: RefCell<Failure> = const {
        RefCell::new(Failure { code: "", message: String::new() })
    };

    /// The bytes the last synchronous call on this thread handed out.
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Runs `body`, catching a panic as an `INTERNAL` failure.
pub(crate) fn caught<T>(body: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|panic| {
        Err(Failure {
            code: "INTERNAL",
            message: panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "the engine panicked".to_owned()),
        })
    })
}

/// Runs `body`, turning a failure or a panic into `-1` and keeping it for
/// [`darudb_last_error`].
fn guard(body: impl FnOnce() -> Result<i32>) -> i32 {
    match caught(body) {
        Ok(status) => status,
        Err(failure) => {
            LAST_ERROR.with_borrow_mut(|last| *last = failure);

            -1
        }
    }
}

/// Hands out the bytes `fill` writes into this thread's buffer, through
/// `out`.
///
/// # Safety
///
/// `out` points to a `Buf` the caller can write.
unsafe fn hand_out(out: *mut Buf, fill: impl FnOnce(&mut Vec<u8>) -> Result<i32>) -> Result<i32> {
    OUT.with_borrow_mut(|bytes| {
        bytes.clear();

        let status = fill(bytes)?;

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

        Ok(status)
    })
}

/// The `len` bytes at `ptr`, or none when `len` is 0.
///
/// # Safety
///
/// `ptr` points to `len` readable bytes that do not change during the call.
pub(crate) unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
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
pub(crate) unsafe fn text<'a>(ptr: *const u8, len: usize) -> Result<&'a str> {
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

/// The value a handle points to.
///
/// # Safety
///
/// `handle` is null, or came from this library and has not been freed.
pub(crate) unsafe fn at<'a, T>(handle: *const T, gone: fn() -> Failure) -> Result<&'a T> {
    // SAFETY: the caller's promise; null is refused.
    unsafe { handle.as_ref() }.ok_or_else(gone)
}

/// A new reference to the value an `Arc` handle points to, for work that
/// outlives the call.
///
/// # Safety
///
/// As [`at`], with `handle` made by `Arc::into_raw`.
pub(crate) unsafe fn share<T>(handle: *const T, gone: fn() -> Failure) -> Result<Arc<T>> {
    if handle.is_null() {
        return Err(gone());
    }

    // SAFETY: the caller promises the handle is a live `Arc` this library
    // made; one more strong count is taken, for the `Arc` made below.
    unsafe { Arc::increment_strong_count(handle) };

    // SAFETY: the count taken above is the one this `Arc` owns.
    Ok(unsafe { Arc::from_raw(handle) })
}

/// Frees an `Arc` handle.
///
/// # Safety
///
/// `handle` is null, or came from this library and is not used after.
unsafe fn release<T>(handle: *const T) {
    if !handle.is_null() {
        // SAFETY: the caller hands the reference back, once.
        drop(unsafe { Arc::from_raw(handle) });
    }
}

fn gone_prepared() -> Failure {
    invalid("a prepared query that is gone")
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
        // SAFETY: the caller promises `code` is writable or null; the code
        // is static.
        unsafe {
            put(
                code,
                Buf {
                    ptr: last.code.as_ptr(),
                    len: last.code.len(),
                },
            );
        }
        // SAFETY: the caller promises `message` is writable or null; the
        // message lives until the next failure.
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
    database: *mut *const Database,
    migration: *mut *const Held,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `path`.
        let path = unsafe { text(path, path_len) }?;
        // SAFETY: the caller's promise for `options`.
        let options = unsafe { bytes(options, options_len) };

        match ops::open(path, options)? {
            ops::Opened::Database(opened) => {
                // SAFETY: the caller's promise for `database`.
                unsafe { put(database, opened) };

                Ok(0)
            }
            ops::Opened::Migration(pending) => {
                // SAFETY: the caller's promise for `migration`.
                unsafe { put(migration, pending) };

                Ok(1)
            }
        }
    })
}

/// Frees a database handle. A database that is still open closes when the
/// last call that uses it has returned. Its transactions live on.
///
/// # Safety
///
/// `database` came from this library and is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_database_free(database: *const Database) {
    // SAFETY: the caller's promise.
    unsafe { release(database) };
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
        let database = unsafe { at(database, closed) }?;

        ops::close(database)
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
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;
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
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;

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
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;
        let read = ops::begin_read(database)?;

        // SAFETY: the caller's promise for `txn`.
        unsafe { put(txn, read) };

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
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;
        let write = ops::begin_write(database)?;

        // SAFETY: the caller's promise for `txn`.
        unsafe { put(txn, write) };

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
        ops::sync(unsafe { at(database, closed) }?)
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
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;

        // SAFETY: the caller's promise for `key`.
        ops::set_key(database, unsafe { bytes(key, key_len) })
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
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;

        // SAFETY: the caller's promise for `password`.
        ops::set_password(database, unsafe { bytes(password, password_len) })
    })
}

/// Frees a transaction handle, ending the transaction if it has not ended
/// and no asynchronous call still holds it: a write's changes are thrown
/// away.
///
/// # Safety
///
/// `txn` came from this library and is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_txn_free(txn: *const Held) {
    // SAFETY: the caller's promise.
    unsafe { release(txn) };
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
    if let Ok(held) = unsafe { at(txn, ops::ended) } {
        held.end();
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
        ops::commit(unsafe { at(txn, ops::ended) }?, deferred != 0)
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

fn no_name() -> Failure {
    invalid("a collection without a name")
}

/// The name a handle points to.
///
/// # Safety
///
/// `name` came from [`darudb_name`] and has not been freed.
pub(crate) unsafe fn name_at<'a>(name: *const String) -> Result<&'a str> {
    // SAFETY: the caller's promise.
    unsafe { at(name, no_name) }.map(String::as_str)
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::get(held, name, key, bytes)) }
    })
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `ir`.
        let request = ops::request(unsafe { bytes(ir, ir_len) }, first != 0)?;

        // SAFETY: the caller's promise for `out`.
        unsafe {
            hand_out(out, |bytes| {
                ops::find(held, &request.collection, &request.query, bytes)
            })
        }
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `ir`.
        let request = ops::request(unsafe { bytes(ir, ir_len) }, false)?;
        let found = ops::count(held, &request.collection, &request.query)?;

        // SAFETY: the caller's promise for `count`.
        unsafe { put(count, found) };

        Ok(0)
    })
}

/// Prepares the query in the query language at `query` on collection
/// `name`, into `prepared`.
///
/// # Safety
///
/// `name` and `query` point to as many readable bytes as their lengths say;
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
        let name = unsafe { text(name, name_len) }?;
        // SAFETY: the caller's promise for `query`.
        let query = unsafe { text(query, query_len) }?;
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
        // SAFETY: the caller's promise for `ir`.
        let request = ops::request(unsafe { bytes(ir, ir_len) }, false)?;
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
    // SAFETY: the caller's promise.
    unsafe { release(prepared) };
}

/// The records a prepared query finds with the parameters' values in the
/// record at `parameters`, as [`darudb_find`] gives them.
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `prepared`.
        let prepared = unsafe { at(prepared, gone_prepared) }?;
        // SAFETY: the caller's promise for `parameters`.
        let query = prepared.bound(unsafe { bytes(parameters, parameters_len) }, first != 0)?;

        // SAFETY: the caller's promise for `out`.
        unsafe {
            hand_out(out, |bytes| {
                ops::find(held, &prepared.collection, &query, bytes)
            })
        }
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `prepared`.
        let prepared = unsafe { at(prepared, gone_prepared) }?;
        // SAFETY: the caller's promise for `parameters`.
        let query = prepared.bound(unsafe { bytes(parameters, parameters_len) }, false)?;
        let found = ops::count(held, &prepared.collection, &query)?;

        // SAFETY: the caller's promise for `count`.
        unsafe { put(count, found) };

        Ok(0)
    })
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name) }?;
        // SAFETY: the caller's promise for `records`.
        let records = unsafe { bytes(records, records_len) };

        // SAFETY: the caller's promise for `out`.
        unsafe {
            hand_out(out, |keys| {
                ops::write(held, name, records, replace != 0, keys)
            })
        }
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        // SAFETY: the caller's promise for `changes`.
        ops::update(held, name, key, unsafe { bytes(changes, changes_len) })
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        ops::delete(held, name, key)
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
        // SAFETY: the caller's promise for `txn`.
        let found = ops::previous_version(unsafe { at(txn, ops::ended) }?)?;

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
        // SAFETY: the caller's promise for `txn`.
        let held = unsafe { at(txn, ops::ended) }?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::schema_record(held, previous != 0, bytes)) }
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
        // SAFETY: the caller's promise for `txn`.
        match ops::next_step(unsafe { at(txn, ops::ended) }?)? {
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::previous_record(held, name, key, bytes)) }
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
        let held = unsafe { at(txn, ops::ended) }?;
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_at(name) }?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::previous_keys(held, name, bytes)) }
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
    database: *mut *const Database,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `txn`.
        let opened = ops::finish(unsafe { at(txn, ops::ended) }?)?;

        // SAFETY: the caller's promise for `database`.
        unsafe { put(database, opened) };

        Ok(0)
    })
}

/// The integrity check of the published commit: its report, as a record,
/// in `out`. Returns 1 if it found no problem, and 0 otherwise.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `out` points
/// to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_check(database: *const Database, out: *mut Buf) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::check(database, bytes)) }
    })
}

/// Writes a copy of the published commit to a new file at `path`: its
/// report, as a record, in `out`.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `path` points
/// to `path_len` readable bytes; `out` to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_backup(
    database: *const Database,
    path: *const u8,
    path_len: usize,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;
        // SAFETY: the caller's promise for `path`.
        let path = unsafe { text(path, path_len) }?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::backup(database, path, bytes)) }
    })
}

/// Makes the file smaller in place: its report, as a record, in `out`.
///
/// # Safety
///
/// `database` came from this library and has not been freed; `out` points
/// to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_compact(database: *const Database, out: *mut Buf) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `database`.
        let database = unsafe { at(database, closed) }?;

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::compact(database, bytes)) }
    })
}

/// Rescues what it can of the file at `from` into a new file at `into`,
/// with the options in the record at `options`: its report, as a record, in
/// `out`. Returns 1 if the new file holds the commit whole, and 0 otherwise.
///
/// # Safety
///
/// `from`, `into` and `options` point to as many readable bytes as their
/// lengths say; `out` to a `Buf` the caller can write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_salvage(
    from: *const u8,
    from_len: usize,
    into: *const u8,
    into_len: usize,
    options: *const u8,
    options_len: usize,
    out: *mut Buf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller's promise for `from`.
        let from = unsafe { text(from, from_len) }?;
        // SAFETY: the caller's promise for `into`.
        let into = unsafe { text(into, into_len) }?;
        // SAFETY: the caller's promise for `options`.
        let options = unsafe { bytes(options, options_len) };

        // SAFETY: the caller's promise for `out`.
        unsafe { hand_out(out, |bytes| ops::salvage(from, into, options, bytes)) }
    })
}
