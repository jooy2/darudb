//! The asynchronous calls: each takes what it needs, copies its bytes and
//! takes a reference of its own to each handle, runs on a thread of this
//! library, and hands its result to a Dart callback, which `dart:ffi`'s
//! `NativeCallable.listener` runs on the isolate's event loop.
//!
//! The threads are a pool that grows when every thread is busy and shrinks
//! when one has had nothing to do for a while. A fixed number of threads
//! could all wait for a writer whose own next call needs a thread, which
//! would wait for ever; the pool grows instead, up to [`MOST_THREADS`].
//!
//! A result is a status, as a synchronous call returns, and bytes the
//! library allocated, which the Dart side frees with [`darudb_buffer_free`]
//! once it has read them: a handle a call made is its address in eight
//! bytes, little-endian, a count eight bytes too, and a failure its code, a
//! zero byte and its message.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use zeroize::Zeroizing;

use crate::ops::{self, Database, Held, Opened, Prepared};
use crate::{Failure, Result, bytes, caught, closed, invalid, name_at, share, text};

/// What the Dart side gives to be called with a result: the call's id, its
/// status, and the bytes, which it frees.
pub type Callback = extern "C" fn(id: i64, status: i32, data: *mut u8, len: usize);

/// The most threads the pool keeps at once.
const MOST_THREADS: usize = 64;

/// How long a thread with nothing to do waits for work before it ends.
const IDLE: Duration = Duration::from_secs(10);

type Job = Box<dyn FnOnce() + Send>;

struct Pool {
    state: Mutex<State>,
    ready: Condvar,
}

struct State {
    jobs: VecDeque<Job>,
    idle: usize,
    threads: usize,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();

    POOL.get_or_init(|| Pool {
        state: Mutex::new(State {
            jobs: VecDeque::new(),
            idle: 0,
            threads: 0,
        }),
        ready: Condvar::new(),
    })
}

/// Runs `job` on a thread of the pool.
fn spawn(job: Job) {
    let pool = pool();
    let mut state = pool.state.lock().unwrap_or_else(PoisonError::into_inner);

    state.jobs.push_back(job);

    if state.idle > 0 {
        pool.ready.notify_one();
    } else if state.threads < MOST_THREADS {
        let started = std::thread::Builder::new()
            .name("darudb-dart".to_owned())
            .spawn(work);

        // A thread that does not start leaves the job queued for the threads
        // there are.
        if started.is_ok() {
            state.threads += 1;
        }
    }
}

/// What a thread of the pool does: runs jobs until it has had none for
/// [`IDLE`].
fn work() {
    let pool = pool();
    let mut state = pool.state.lock().unwrap_or_else(PoisonError::into_inner);

    loop {
        if let Some(job) = state.jobs.pop_front() {
            drop(state);
            job();
            state = pool.state.lock().unwrap_or_else(PoisonError::into_inner);

            continue;
        }

        state.idle += 1;

        let (next, waited) = pool
            .ready
            .wait_timeout(state, IDLE)
            .unwrap_or_else(PoisonError::into_inner);

        state = next;
        state.idle -= 1;

        if waited.timed_out() && state.jobs.is_empty() {
            state.threads -= 1;

            return;
        }
    }
}

/// Runs `operation` on the pool, and hands Dart its status and the bytes it
/// wrote, or its failure.
fn submit(
    id: i64,
    callback: Callback,
    operation: impl FnOnce(&mut Vec<u8>) -> Result<i32> + Send + 'static,
) {
    spawn(Box::new(move || {
        let mut out = Vec::new();
        let status = match caught(|| operation(&mut out)) {
            Ok(status) => status,
            Err(failure) => {
                out.clear();
                out.extend_from_slice(failure.code.as_bytes());
                out.push(0);
                out.extend_from_slice(failure.message.as_bytes());

                -1
            }
        };
        let (data, len) = if out.is_empty() {
            (std::ptr::null_mut(), 0)
        } else {
            let len = out.len();

            (Box::into_raw(out.into_boxed_slice()).cast::<u8>(), len)
        };

        callback(id, status, data, len);
    }));
}

/// Reports `failure` to Dart without running anything, for a call whose
/// arguments do not hold.
fn refuse(id: i64, callback: Callback, failure: Failure) {
    submit(id, callback, move |_| Err(failure));
}

/// Runs the asynchronous call built by `prepare`, which reads the call's
/// arguments, refusing it if they do not hold.
fn call<F>(id: i64, callback: Callback, prepare: impl FnOnce() -> Result<F>)
where
    F: FnOnce(&mut Vec<u8>) -> Result<i32> + Send + 'static,
{
    match caught(prepare) {
        Ok(operation) => submit(id, callback, operation),
        Err(failure) => refuse(id, callback, failure),
    }
}

/// A handle's address, as the bytes of a result.
fn address<T>(out: &mut Vec<u8>, handle: *const T) {
    out.extend_from_slice(&(handle as usize as u64).to_le_bytes());
}

/// Frees the bytes of an asynchronous result.
///
/// # Safety
///
/// `data` and `len` are what a callback was given, and are not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_buffer_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        // SAFETY: the caller hands back the box `submit` leaked, once, with
        // its length.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len)) });
    }
}

/// [`crate::darudb_open`] on a thread of the library: status 0 with the
/// database's handle, or 1 with the migration's.
///
/// # Safety
///
/// As [`crate::darudb_open`], without the handles to write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_open_async(
    path: *const u8,
    path_len: usize,
    options: *const u8,
    options_len: usize,
    id: i64,
    callback: Callback,
) {
    call(id, callback, || {
        // SAFETY: the caller's promise for `path`.
        let path = unsafe { text(path, path_len) }?.to_owned();
        // SAFETY: the caller's promise for `options`; the copy may hold a
        // key or a password, and is wiped when dropped.
        let options = Zeroizing::new(unsafe { bytes(options, options_len) }.to_vec());

        Ok(move |out: &mut Vec<u8>| match ops::open(&path, &options)? {
            Opened::Database(opened) => {
                address(out, opened);

                Ok(0)
            }
            Opened::Migration(pending) => {
                address(out, pending);

                Ok(1)
            }
        })
    });
}

/// [`crate::darudb_begin_write`] on a thread of the library: the
/// transaction's handle.
///
/// # Safety
///
/// `database` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_begin_write_async(
    database: *const Database,
    id: i64,
    callback: Callback,
) {
    call(id, callback, || {
        // SAFETY: the caller's promise.
        let database = unsafe { share(database, closed) }?;

        Ok(move |out: &mut Vec<u8>| {
            address(out, ops::begin_write(&database)?);

            Ok(0)
        })
    });
}

/// [`crate::darudb_begin_read`] on a thread of the library: the
/// transaction's handle.
///
/// # Safety
///
/// As [`darudb_begin_write_async`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_begin_read_async(
    database: *const Database,
    id: i64,
    callback: Callback,
) {
    call(id, callback, || {
        // SAFETY: the caller's promise.
        let database = unsafe { share(database, closed) }?;

        Ok(move |out: &mut Vec<u8>| {
            address(out, ops::begin_read(&database)?);

            Ok(0)
        })
    });
}

/// Runs `operation` on the database on a thread of the library.
///
/// # Safety
///
/// As [`darudb_begin_write_async`].
unsafe fn on_database(
    database: *const Database,
    id: i64,
    callback: Callback,
    operation: impl FnOnce(&Database) -> Result<i32> + Send + 'static,
) {
    call(id, callback, || {
        // SAFETY: the caller's promise.
        let database = unsafe { share(database, closed) }?;

        Ok(move |_: &mut Vec<u8>| operation(&database))
    });
}

/// [`crate::darudb_sync`] on a thread of the library.
///
/// # Safety
///
/// As [`darudb_begin_write_async`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_sync_async(database: *const Database, id: i64, callback: Callback) {
    // SAFETY: the caller's promise.
    unsafe { on_database(database, id, callback, ops::sync) };
}

/// [`crate::darudb_close`] on a thread of the library.
///
/// # Safety
///
/// As [`darudb_begin_write_async`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_close_async(
    database: *const Database,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise.
    unsafe { on_database(database, id, callback, ops::close) };
}

/// [`crate::darudb_set_key`] on a thread of the library.
///
/// # Safety
///
/// As [`crate::darudb_set_key`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_set_key_async(
    database: *const Database,
    key: *const u8,
    key_len: usize,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise for `key`; the copy is wiped when dropped.
    let key = Zeroizing::new(unsafe { bytes(key, key_len) }.to_vec());

    // SAFETY: the caller's promise for `database`.
    unsafe {
        on_database(database, id, callback, move |database| {
            ops::set_key(database, &key)
        })
    };
}

/// [`crate::darudb_set_password`] on a thread of the library.
///
/// # Safety
///
/// As [`crate::darudb_set_password`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_set_password_async(
    database: *const Database,
    password: *const u8,
    password_len: usize,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise for `password`; the copy is wiped when
    // dropped.
    let password = Zeroizing::new(unsafe { bytes(password, password_len) }.to_vec());

    // SAFETY: the caller's promise for `database`.
    unsafe {
        on_database(database, id, callback, move |database| {
            ops::set_password(database, &password)
        });
    }
}

/// Runs `operation` on the transaction on a thread of the library.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
unsafe fn on_txn(
    txn: *const Held,
    id: i64,
    callback: Callback,
    operation: impl FnOnce(&Held, &mut Vec<u8>) -> Result<i32> + Send + 'static,
) {
    call(id, callback, || {
        // SAFETY: the caller's promise.
        let held: Arc<Held> = unsafe { share(txn, ops::ended) }?;

        Ok(move |out: &mut Vec<u8>| operation(&held, out))
    });
}

/// [`crate::darudb_commit`] on a thread of the library.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_commit_async(
    txn: *const Held,
    deferred: u8,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise.
    unsafe {
        on_txn(txn, id, callback, move |held, _| {
            ops::commit(held, deferred != 0)
        })
    };
}

/// The name a handle points to, copied for work that outlives the call.
///
/// # Safety
///
/// As [`name_at`].
unsafe fn name_of(name: *const String) -> Result<String> {
    // SAFETY: the caller's promise.
    unsafe { name_at(name) }.map(str::to_owned)
}

/// [`crate::darudb_get`] on a thread of the library: status 1 with the
/// record, or 0.
///
/// # Safety
///
/// As [`crate::darudb_get`], without `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_get_async(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    id: i64,
    callback: Callback,
) {
    let arguments = || {
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_of(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        Ok((name, key))
    };

    match caught(arguments) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, key)) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                ops::get(held, &name, key, out)
            })
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_find`] on a thread of the library.
///
/// # Safety
///
/// As [`crate::darudb_find`], without `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_find_async(
    txn: *const Held,
    ir: *const u8,
    ir_len: usize,
    first: u8,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise for `ir`.
    match caught(|| ops::request(unsafe { bytes(ir, ir_len) }, first != 0)) {
        // SAFETY: the caller's promise for `txn`.
        Ok(request) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                ops::find(held, &request.collection, &request.query, out)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_count`] on a thread of the library: the count.
///
/// # Safety
///
/// As [`crate::darudb_count`], without `count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_count_async(
    txn: *const Held,
    ir: *const u8,
    ir_len: usize,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise for `ir`.
    match caught(|| ops::request(unsafe { bytes(ir, ir_len) }, false)) {
        // SAFETY: the caller's promise for `txn`.
        Ok(request) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                let found = ops::count(held, &request.collection, &request.query)?;

                out.extend_from_slice(&found.to_le_bytes());

                Ok(0)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// The prepared query a handle points to, bound to the values at
/// `parameters`, with the collection it is on.
///
/// # Safety
///
/// `prepared` came from this library and has not been freed; `parameters`
/// points to `parameters_len` readable bytes.
unsafe fn bound(
    prepared: *const Prepared,
    parameters: *const u8,
    parameters_len: usize,
    first: bool,
) -> Result<(String, darudb::Query)> {
    // SAFETY: the caller's promise for `prepared`.
    let prepared = unsafe { crate::at(prepared, || invalid("a prepared query that is gone")) }?;
    // SAFETY: the caller's promise for `parameters`.
    let query = prepared.bound(unsafe { bytes(parameters, parameters_len) }, first)?;

    Ok((prepared.collection.clone(), query))
}

/// [`crate::darudb_find_prepared`] on a thread of the library.
///
/// # Safety
///
/// As [`crate::darudb_find_prepared`], without `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_find_prepared_async(
    txn: *const Held,
    prepared: *const Prepared,
    parameters: *const u8,
    parameters_len: usize,
    first: u8,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promises for `prepared` and `parameters`.
    match caught(|| unsafe { bound(prepared, parameters, parameters_len, first != 0) }) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, query)) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                ops::find(held, &name, &query, out)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_count_prepared`] on a thread of the library: the count.
///
/// # Safety
///
/// As [`crate::darudb_count_prepared`], without `count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_count_prepared_async(
    txn: *const Held,
    prepared: *const Prepared,
    parameters: *const u8,
    parameters_len: usize,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promises for `prepared` and `parameters`.
    match caught(|| unsafe { bound(prepared, parameters, parameters_len, false) }) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, query)) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                out.extend_from_slice(&ops::count(held, &name, &query)?.to_le_bytes());

                Ok(0)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_write`] on a thread of the library: the keys.
///
/// # Safety
///
/// As [`crate::darudb_write`], without `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_write_async(
    txn: *const Held,
    name: *const String,
    records: *const u8,
    records_len: usize,
    replace: u8,
    id: i64,
    callback: Callback,
) {
    let arguments = || {
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_of(name) }?;
        // SAFETY: the caller's promise for `records`.
        let records = unsafe { bytes(records, records_len) }.to_vec();

        Ok((name, records))
    };

    match caught(arguments) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, records)) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                ops::write(held, &name, &records, replace != 0, out)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_update`] on a thread of the library.
///
/// # Safety
///
/// As [`crate::darudb_update`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_update_async(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    changes: *const u8,
    changes_len: usize,
    id: i64,
    callback: Callback,
) {
    let arguments = || {
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_of(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;
        // SAFETY: the caller's promise for `changes`.
        let changes = unsafe { bytes(changes, changes_len) }.to_vec();

        Ok((name, key, changes))
    };

    match caught(arguments) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, key, changes)) => unsafe {
            on_txn(txn, id, callback, move |held, _| {
                ops::update(held, &name, key, &changes)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_delete`] on a thread of the library.
///
/// # Safety
///
/// As [`crate::darudb_delete`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_delete_async(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    id: i64,
    callback: Callback,
) {
    let arguments = || {
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_of(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        Ok((name, key))
    };

    match caught(arguments) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, key)) => unsafe {
            on_txn(txn, id, callback, move |held, _| {
                ops::delete(held, &name, key)
            })
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_migration_next_step`] on a thread of the library: status
/// 1 with the step's version, or 0 once every step has run.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_next_step_async(
    txn: *const Held,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise.
    unsafe {
        on_txn(txn, id, callback, |held, out| match ops::next_step(held)? {
            Some(step) => {
                out.extend_from_slice(&step.to_le_bytes());

                Ok(1)
            }
            None => Ok(0),
        });
    }
}

/// [`crate::darudb_migration_finish`] on a thread of the library: the open
/// database's handle.
///
/// # Safety
///
/// `txn` came from this library and has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_finish_async(
    txn: *const Held,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise.
    unsafe {
        on_txn(txn, id, callback, |held, out| {
            address(out, ops::finish(held)?);

            Ok(0)
        });
    }
}

/// [`crate::darudb_migration_previous_record`] on a thread of the library.
///
/// # Safety
///
/// As [`darudb_get_async`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_previous_record_async(
    txn: *const Held,
    name: *const String,
    key: *const u8,
    key_len: usize,
    id: i64,
    callback: Callback,
) {
    let arguments = || {
        // SAFETY: the caller's promise for `name`.
        let name = unsafe { name_of(name) }?;
        // SAFETY: the caller's promise for `key`.
        let key = ops::key(unsafe { bytes(key, key_len) })?;

        Ok((name, key))
    };

    match caught(arguments) {
        // SAFETY: the caller's promise for `txn`.
        Ok((name, key)) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                ops::previous_record(held, &name, key, out)
            });
        },
        Err(failure) => refuse(id, callback, failure),
    }
}

/// [`crate::darudb_migration_previous_keys`] on a thread of the library.
///
/// # Safety
///
/// `txn` and `name` came from this library and have not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn darudb_migration_previous_keys_async(
    txn: *const Held,
    name: *const String,
    id: i64,
    callback: Callback,
) {
    // SAFETY: the caller's promise for `name`.
    match caught(|| unsafe { name_of(name) }) {
        // SAFETY: the caller's promise for `txn`.
        Ok(name) => unsafe {
            on_txn(txn, id, callback, move |held, out| {
                ops::previous_keys(held, &name, out)
            })
        },
        Err(failure) => refuse(id, callback, failure),
    }
}
