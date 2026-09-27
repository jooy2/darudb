//! The native half of the Node.js binding of DaruDB.
//!
//! This crate translates between JavaScript and the engine in `crates/darudb`
//! and decides nothing of its own: every rule about a database file lives in
//! the engine, so that Node.js and every other language read a file the same
//! way.
//!
//! `npm run build` generates `native.js` and `native.d.ts` from the
//! `#[napi]` items here. They are the package's internals, not its API: the
//! JavaScript in `lib/` wraps them in the API a user sees, declared by hand in
//! `index.d.ts`. Objects cross as records and queries as IR, the byte formats
//! of `design/objects.md`, which `lib/codec.js` writes and reads, so that a
//! batch of objects costs one call and one buffer rather than a call per
//! field. A prepared query is parsed once into a [`NativePrepared`], which
//! the synchronous methods run with its parameters' values.
//!
//! Every operation of a transaction has a synchronous method, and the
//! asynchronous API runs them in batches through `run_async`, on the libuv
//! thread pool, resolving a promise, so the event loop never waits for the
//! disk. Opening a database, beginning a write transaction, committing and
//! closing have `…Async` methods of their own. A transaction sits behind a
//! mutex so that a batch can take it to whichever pool thread runs it; the
//! JavaScript side sends one batch of a transaction at a time, with its
//! operations in the order they were called, so the mutex is never
//! contended.
//!
//! An error thrown from here is a JavaScript `Error` whose `code` is the
//! engine's [`darudb::Error::code`], unchanged. A pool thread cannot throw, so
//! an asynchronous operation that fails resolves to a [`NativeFailure`], which
//! the JavaScript side throws.

use std::sync::{Arc, Mutex, MutexGuard};

use napi::bindgen_prelude::{
    AsyncTask, BigInt, Buffer, BufferSlice, Either, Either4, External, FromNapiValue, Null,
    ToNapiValue, TypeName, Uint8Array,
};
use napi::{Env, Task, Unknown, ValueType};
use napi_derive::napi;

/// A result whose error becomes a JavaScript `Error` with the engine's code.
///
/// It has to be named `Result`: `#[napi]` recognises a fallible function by
/// the name of its return type, and treats any other name as a value to
/// convert.
type Result<T> = napi::Result<T, &'static str>;

/// A primary key as JavaScript gets it back: a number, or a `bigint` when it
/// is beyond what a number holds exactly, a string, or bytes.
type JsKeyOut = Either4<f64, BigInt, String, Buffer>;

/// The largest integer a JavaScript number holds, with every smaller one.
const SAFE_INTEGER: i64 = (1 << 53) - 1;

/// The file format version this build of the engine reads and writes.
#[napi]
pub const FORMAT_VERSION: u32 = darudb::FORMAT_VERSION;

/// The version of the DaruDB engine inside this package.
#[napi]
pub fn engine_version() -> &'static str {
    darudb::VERSION
}

/// What an asynchronous operation resolves to when it fails: the error's
/// code and message, which the JavaScript side throws.
#[napi(object)]
pub struct NativeFailure {
    pub code: String,
    pub message: String,
}

/// A migration step's changes that are not a function: what
/// `darudb::Migration` holds besides `run`.
#[napi(object)]
pub struct NativeMigration {
    pub version: u32,
    /// Pairs of the old name and the new.
    pub rename_collections: Vec<Vec<String>>,
    /// Triples of the collection, the old field name and the new.
    pub rename_fields: Vec<Vec<String>>,
    pub delete_collections: Vec<String>,
    /// Pairs of the collection and the field.
    pub replace_fields: Vec<Vec<String>>,
}

/// What `Database.open` passes down.
#[napi(object)]
pub struct NativeOptions {
    pub create: Option<bool>,
    pub page_size: Option<u32>,
    /// How long to wait for another writer, in milliseconds.
    pub busy_timeout: Option<u32>,
    /// The memory the page cache may take, in bytes: a whole number, which
    /// the JavaScript side checks.
    pub cache_size: Option<f64>,
    /// The declared schema, as `Schema::decode` reads it.
    pub schema: Option<Buffer>,
    pub migrations: Option<Vec<NativeMigration>>,
}

/// Work for the thread pool: a function that returns a value or an error.
pub struct Work<T: Deliver> {
    run: Option<Box<dyn FnOnce() -> Result<T> + Send>>,
}

impl<T: Deliver> Work<T> {
    fn task(run: impl FnOnce() -> Result<T> + Send + 'static) -> AsyncTask<Self> {
        AsyncTask::new(Self {
            run: Some(Box::new(run)),
        })
    }
}

/// A value a pool thread hands back, and what JavaScript gets for it.
pub trait Deliver: Send + 'static {
    type Js: ToNapiValue + TypeName;

    fn deliver(self) -> Result<Self::Js>;
}

impl<T: Deliver> Task for Work<T> {
    type Output = std::result::Result<T, NativeFailure>;
    type JsValue = Either<T::Js, NativeFailure>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let run = self
            .run
            .take()
            .ok_or_else(|| napi::Error::from_reason("a pool task ran twice".to_owned()))?;

        Ok(run().map_err(failure))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(
            match output.and_then(|value| value.deliver().map_err(failure)) {
                Ok(js) => Either::A(js),
                Err(failure) => Either::B(failure),
            },
        )
    }
}

fn failure(error: napi::Error<&'static str>) -> NativeFailure {
    NativeFailure {
        code: error.status.to_owned(),
        message: error.reason.clone(),
    }
}

impl Deliver for Vec<u8> {
    type Js = Buffer;

    fn deliver(self) -> Result<Buffer> {
        Ok(self.into())
    }
}

impl Deliver for () {
    type Js = Null;

    fn deliver(self) -> Result<Null> {
        Ok(Null)
    }
}

/// Primary keys, as `insert` and `previousKeys` give them.
pub struct Keys(Vec<darudb::Value>);

impl Deliver for Keys {
    type Js = Vec<JsKeyOut>;

    fn deliver(self) -> Result<Self::Js> {
        self.0.into_iter().map(key_out).collect()
    }
}

impl Deliver for darudb::Database {
    type Js = NativeDatabase;

    fn deliver(self) -> Result<NativeDatabase> {
        Ok(NativeDatabase { inner: Some(self) })
    }
}

impl Deliver for darudb::Opening {
    type Js = NativeOpening;

    fn deliver(self) -> Result<NativeOpening> {
        Ok(NativeOpening { state: Some(self) })
    }
}

impl Deliver for Txn {
    type Js = NativeTransaction;

    fn deliver(self) -> Result<NativeTransaction> {
        Ok(NativeTransaction::of(self))
    }
}

/// The options of `Database.open`, as the engine takes them.
fn open_options(options: NativeOptions) -> Result<darudb::OpenOptions> {
    let mut open_options = darudb::OpenOptions::new();

    if let Some(create) = options.create {
        open_options.create(create);
    }

    if let Some(page_size) = options.page_size {
        open_options.page_size(page_size);
    }

    if let Some(milliseconds) = options.busy_timeout {
        open_options.busy_timeout(std::time::Duration::from_millis(u64::from(milliseconds)));
    }

    if let Some(bytes) = options.cache_size {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a cast saturates, so a size beyond the address space asks for all of it"
        )]
        let bytes = bytes as usize;

        open_options.cache_size(bytes);
    }

    if let Some(schema) = &options.schema {
        open_options.schema(darudb::Schema::decode(schema).map_err(to_js_error)?);
    }

    for migration in options.migrations.unwrap_or_default() {
        open_options.migration(migration_of(migration)?);
    }

    Ok(open_options)
}

/// Opening a database: the open database, or the migration it is going
/// through.
#[napi]
pub struct NativeOpening {
    state: Option<darudb::Opening>,
}

#[napi]
impl NativeOpening {
    #[napi(factory)]
    pub fn open(path: String, options: NativeOptions) -> Result<Self> {
        let state = open_options(options)?
            .open_migrating(&path)
            .map_err(to_js_error)?;

        Ok(Self { state: Some(state) })
    }

    /// `open` on the thread pool.
    #[napi(ts_return_type = "Promise<NativeOpening | NativeFailure>")]
    pub fn open_async(
        path: String,
        options: NativeOptions,
    ) -> Result<AsyncTask<Work<darudb::Opening>>> {
        let options = open_options(options)?;

        Ok(Work::task(move || {
            options.open_migrating(&path).map_err(to_js_error)
        }))
    }

    /// Whether a migration is under way.
    #[napi(getter)]
    pub fn is_migrating(&self) -> bool {
        matches!(self.state, Some(darudb::Opening::Migrating(_)))
    }

    /// The open database, when no migration is under way.
    #[napi]
    pub fn database(&mut self) -> Result<NativeDatabase> {
        match self.state.take() {
            Some(darudb::Opening::Open(database)) => Ok(NativeDatabase {
                inner: Some(database),
            }),
            _ => Err(invalid("the opening has no open database")),
        }
    }

    /// The migration under way, as a transaction.
    #[napi]
    pub fn migration(&mut self) -> Result<NativeTransaction> {
        match self.state.take() {
            Some(darudb::Opening::Migrating(pending)) => {
                Ok(NativeTransaction::of(Txn::Migration(pending)))
            }
            _ => Err(invalid("the opening has no migration under way")),
        }
    }
}

/// An open database.
#[napi]
pub struct NativeDatabase {
    inner: Option<darudb::Database>,
}

#[napi]
impl NativeDatabase {
    #[napi(getter)]
    pub fn page_size(&self) -> Result<u32> {
        Ok(self.database()?.page_size())
    }

    #[napi(getter)]
    pub fn format_version(&self) -> Result<u32> {
        Ok(self.database()?.format_version())
    }

    /// The stored schema's record, or `null` for a database opened without
    /// a schema.
    #[napi(getter)]
    pub fn schema_record(&self) -> Result<Option<Buffer>> {
        Ok(self
            .database()?
            .schema_record()
            .map(|record| record.to_vec().into()))
    }

    #[napi]
    pub fn begin_read(&self) -> Result<NativeTransaction> {
        let txn = self.database()?.begin_read().map_err(to_js_error)?;

        Ok(NativeTransaction::of(Txn::Read(Box::new(txn))))
    }

    /// `beginRead`, giving the transaction only as its handle, which the
    /// functions of a synchronous transaction take, `endTransaction` among
    /// them. `read` begins a transaction for every call, and an object of
    /// its own for each, with a finalizer, cost the garbage collector more
    /// than the rest of a read transaction begun for one lookup.
    #[napi(ts_return_type = "ExternalObject<'NativeTransaction'>")]
    pub fn begin_read_handle(&self) -> Result<External<Held>> {
        let txn = self.database()?.begin_read().map_err(to_js_error)?;

        Ok(External::new(Arc::new(Mutex::new(Some(Txn::Read(
            Box::new(txn),
        ))))))
    }

    /// Begins a write transaction, and gives it only as its handle; see
    /// `beginReadHandle`.
    #[napi(ts_return_type = "ExternalObject<'NativeTransaction'>")]
    pub fn begin_write_handle(&self) -> Result<External<Held>> {
        let txn = self.database()?.begin_write().map_err(to_js_error)?;

        Ok(External::new(Arc::new(Mutex::new(Some(Txn::Write(
            Box::new(txn),
        ))))))
    }

    /// `begin_write` on the thread pool, which waits there for another
    /// process's writer.
    #[napi(ts_return_type = "Promise<NativeTransaction | NativeFailure>")]
    pub fn begin_write_async(&self) -> Result<AsyncTask<Work<Txn>>> {
        let database = self.database()?.clone();

        Ok(Work::task(move || {
            database
                .begin_write()
                .map(|txn| Txn::Write(Box::new(txn)))
                .map_err(to_js_error)
        }))
    }

    #[napi]
    pub fn sync(&self) -> Result<()> {
        self.database()?.sync().map_err(to_js_error)
    }

    #[napi(ts_return_type = "Promise<null | NativeFailure>")]
    pub fn sync_async(&self) -> Result<AsyncTask<Work<()>>> {
        let database = self.database()?.clone();

        Ok(Work::task(move || database.sync().map_err(to_js_error)))
    }

    /// Makes deferred commits durable and closes the handle. Closing one that
    /// is closed does nothing.
    #[napi]
    pub fn close(&mut self) -> Result<()> {
        match self.inner.take() {
            Some(database) => database.close().map_err(to_js_error),
            None => Ok(()),
        }
    }

    #[napi(ts_return_type = "Promise<null | NativeFailure>")]
    pub fn close_async(&mut self) -> AsyncTask<Work<()>> {
        let database = self.inner.take();

        Work::task(move || match database {
            Some(database) => database.close().map_err(to_js_error),
            None => Ok(()),
        })
    }

    fn database(&self) -> Result<&darudb::Database> {
        self.inner
            .as_ref()
            .ok_or_else(|| to_js_error(darudb::Error::Closed))
    }
}

/// What a transaction object holds: a read or a write transaction, or a
/// migration under way, whose write transaction migration functions use.
/// The transactions are boxed, being many times the size of a migration's
/// handle.
pub enum Txn {
    Read(Box<darudb::ReadTransaction>),
    Write(Box<darudb::WriteTransaction>),
    Migration(darudb::PendingMigration),
}

impl Txn {
    fn get_record(&mut self, collection: &str, key: darudb::Value) -> Result<Option<Vec<u8>>> {
        match self {
            Txn::Read(txn) => txn
                .collection(collection)
                .and_then(|collection| collection.get_record(key)),
            _ => self
                .writing()?
                .collection(collection)
                .and_then(|collection| collection.get_record(key)),
        }
        .map_err(to_js_error)
    }

    /// Gives `visit` the record of the object whose key is `key`, where the
    /// engine lends it, and returns whether there was one.
    fn get_record_with(
        &mut self,
        collection: &str,
        key: darudb::Value,
        visit: &mut dyn FnMut(&[u8]) -> darudb::Result<()>,
    ) -> Result<bool> {
        match self {
            Txn::Read(txn) => txn
                .collection(collection)
                .and_then(|collection| collection.get_record_with(key, visit)),
            _ => self
                .writing()?
                .collection(collection)
                .and_then(|collection| collection.get_record_with(key, visit)),
        }
        .map_err(to_js_error)
    }

    /// The records `query` finds, each after its length, copied once each
    /// from where the engine lends them.
    fn find(&mut self, collection: &str, query: &darudb::Query) -> Result<Vec<u8>> {
        let mut out = Vec::new();

        self.find_with(collection, query, &mut |record| {
            push_varint(&mut out, record.len());
            out.extend_from_slice(record);
        })?;

        Ok(out)
    }

    /// [`find`](Self::find), delivered as [`handed`] says. The records are
    /// gathered in a vector this thread keeps for its next query: a vector
    /// made for each query cost an allocation, and more as it grew for a
    /// large result.
    fn find_into<'env>(
        &mut self,
        env: &'env Env,
        collection: &str,
        query: &darudb::Query,
        scratch: &mut [u8],
    ) -> Result<Either<u32, BufferSlice<'env>>> {
        FOUND.with_borrow_mut(|out| {
            out.clear();
            self.find_with(collection, query, &mut |record| {
                push_varint(out, record.len());
                out.extend_from_slice(record);
            })?;

            match (scratch.get_mut(..out.len()), u32::try_from(out.len())) {
                (Some(room), Ok(len)) => {
                    room.copy_from_slice(out);

                    Ok(Either::A(len))
                }
                // A result too large to copy is handed over, and the next
                // query starts a vector again.
                _ if out.len() > COPY_LIMIT => js_bytes(env, std::mem::take(out)).map(Either::B),
                _ => BufferSlice::copy_from(env, &out[..])
                    .map(Either::B)
                    .map_err(|error| napi::Error::new("INTERNAL", error.reason.clone())),
            }
        })
    }

    /// Gives `push` each record `query` finds, where the engine lends it.
    fn find_with(
        &mut self,
        collection: &str,
        query: &darudb::Query,
        push: &mut dyn FnMut(&[u8]),
    ) -> Result<()> {
        let mut push = |record: &[u8]| {
            push(record);

            Ok(())
        };

        match self {
            Txn::Read(txn) => txn
                .collection(collection)
                .and_then(|collection| collection.query_records_with(query, &mut push)),
            _ => self
                .writing()?
                .collection(collection)
                .and_then(|collection| collection.query_records_with(query, &mut push)),
        }
        .map_err(to_js_error)
    }

    fn count(&mut self, collection: &str, query: &darudb::Query) -> Result<f64> {
        match self {
            Txn::Read(txn) => txn
                .collection(collection)
                .and_then(|collection| collection.count(query)),
            _ => self
                .writing()?
                .collection(collection)
                .and_then(|collection| collection.count(query)),
        }
        .map(u64_number)
        .map_err(to_js_error)
    }

    /// The records the query in `ir` finds; only the first with `first`.
    fn find_ir(&mut self, ir: &[u8], first: bool) -> Result<Vec<u8>> {
        let request = request_of(ir, first)?;

        self.find(&request.collection, &request.query)
    }

    /// [`find_ir`](Self::find_ir), delivered as
    /// [`find_into`](Self::find_into) delivers.
    fn find_ir_into<'env>(
        &mut self,
        env: &'env Env,
        ir: &[u8],
        first: bool,
        scratch: &mut [u8],
    ) -> Result<Either<u32, BufferSlice<'env>>> {
        let request = request_of(ir, first)?;

        self.find_into(env, &request.collection, &request.query, scratch)
    }

    fn count_ir(&mut self, ir: &[u8]) -> Result<f64> {
        let request = darudb::QueryRequest::decode(ir).map_err(to_js_error)?;

        self.count(&request.collection, &request.query)
    }

    /// Writes the records in `records`, each after its length, and returns
    /// their keys. A refused record stops the batch there, and the records
    /// before it stay written.
    fn write_records(&mut self, collection: &str, records: &[u8], replace: bool) -> Result<Keys> {
        let mut writer = self
            .writing()?
            .collection(collection)
            .map_err(to_js_error)?;
        let mut keys = Vec::new();
        let mut at = 0;

        while at < records.len() {
            let (len, used) = varint(&records[at..])?;
            let start = at + used;
            let end = start
                .checked_add(len)
                .filter(|end| *end <= records.len())
                .ok_or_else(|| invalid("a batch of records ends inside one"))?;
            let record = &records[start..end];
            let key = if replace {
                writer.put_record(record)
            } else {
                writer.insert_record(record)
            }
            .map_err(to_js_error)?;

            keys.push(key);
            at = end;
        }

        Ok(Keys(keys))
    }

    fn delete(&mut self, collection: &str, key: darudb::Value) -> Result<bool> {
        self.writing()?
            .collection(collection)
            .and_then(|mut collection| collection.delete(key))
            .map_err(to_js_error)
    }

    fn previous_record(&mut self, collection: &str, key: darudb::Value) -> Result<Option<Vec<u8>>> {
        self.migration()?
            .migrating()
            .previous_record(collection, key)
            .map_err(to_js_error)
    }

    fn previous_keys(&mut self, collection: &str) -> Result<Keys> {
        self.migration()?
            .migrating()
            .previous_keys(collection)
            .map(Keys)
            .map_err(to_js_error)
    }

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
}

/// What a transaction object holds, and shares with its handle.
type Held = Arc<Mutex<Option<Txn>>>;

/// A transaction, or a migration under way.
#[napi]
pub struct NativeTransaction {
    held: Held,
}

#[napi]
impl NativeTransaction {
    fn of(txn: Txn) -> Self {
        Self {
            held: Arc::new(Mutex::new(Some(txn))),
        }
    }

    /// The transaction as the functions that make the calls a transaction
    /// makes most take it: `getRecord`, `find`, `writeRecord` and the rest.
    /// A method call costs napi-rs an unwrap of the object and a registration
    /// of the borrow in a map behind a lock, about 80 nanoseconds more than a
    /// function given this handle.
    #[napi(getter, ts_return_type = "ExternalObject<'NativeTransaction'>")]
    pub fn handle(&self) -> External<Held> {
        External::new(Arc::clone(&self.held))
    }

    /// Runs `operation` on the transaction here.
    fn now<T>(&self, operation: impl FnOnce(&mut Txn) -> Result<T>) -> Result<T> {
        with(&self.held, operation)
    }

    /// Runs `operation` on the transaction on the thread pool.
    fn later<T: Deliver>(
        &self,
        operation: impl FnOnce(&mut Txn) -> Result<T> + Send + 'static,
    ) -> AsyncTask<Work<T>> {
        let held = Arc::clone(&self.held);

        Work::task(move || with(&held, operation))
    }

    /// Takes the transaction out, ending it here.
    fn take(&self) -> Result<Txn> {
        lock(&self.held)?.take().ok_or_else(ended)
    }

    /// Runs a batch of operations on the thread pool, one after another in
    /// the order given, and resolves to their results in one buffer, as
    /// [`Batch`] describes. `kinds` holds one of the `OP_` numbers per
    /// operation, and the other arrays what each takes, or an empty string or
    /// `null` where it takes nothing.
    ///
    /// One failing operation fails only its own result, as it would alone:
    /// an operation the engine refuses changes nothing, and the next one
    /// runs.
    #[napi(ts_return_type = "Promise<Buffer | NativeFailure>")]
    pub fn run_async(
        &self,
        kinds: Buffer,
        collections: Vec<String>,
        #[napi(ts_arg_type = "Array<number | bigint | string | Uint8Array | undefined | null>")]
        keys: Vec<Option<Unknown<'_>>>,
        payloads: Vec<Option<Buffer>>,
    ) -> Result<AsyncTask<Work<Vec<u8>>>> {
        if collections.len() != kinds.len()
            || keys.len() != kinds.len()
            || payloads.len() != kinds.len()
        {
            return Err(invalid("a batch's arrays differ in length"));
        }

        let ops: Vec<Op> = kinds
            .iter()
            .zip(collections)
            .zip(keys)
            .zip(payloads)
            .map(|(((kind, collection), key), payload)| Op::of(*kind, collection, key, payload))
            .collect();

        Ok(self.later(move |txn| {
            let mut batch = Batch::default();

            for op in ops {
                batch.push(op.run(txn));
            }

            Ok(batch.0)
        }))
    }

    /// Commits a write transaction, deferred or not, and ends it.
    #[napi]
    pub fn commit(&self, deferred: bool) -> Result<()> {
        commit(self.take()?, deferred)
    }

    #[napi(ts_return_type = "Promise<null | NativeFailure>")]
    pub fn commit_async(&self, deferred: bool) -> Result<AsyncTask<Work<()>>> {
        let txn = self.take()?;

        Ok(Work::task(move || commit(txn, deferred)))
    }

    /// Ends the transaction, throwing a write's changes away. Ending one that
    /// has ended does nothing.
    #[napi]
    pub fn end(&self) {
        if let Ok(mut held) = lock(&self.held) {
            *held = None;
        }
    }

    #[napi(getter)]
    pub fn previous_version(&self) -> Result<f64> {
        self.now(|txn| Ok(u64_number(txn.migration()?.previous_version())))
    }

    /// The record of the schema the migration leads to.
    #[napi(getter)]
    pub fn schema_record(&self) -> Result<Buffer> {
        self.now(|txn| Ok(txn.migration()?.schema_record().to_vec().into()))
    }

    #[napi(getter)]
    pub fn previous_schema_record(&self) -> Result<Buffer> {
        self.now(|txn| Ok(txn.migration()?.previous_schema_record().into()))
    }

    /// Runs the next version step's own function, and returns its version,
    /// or `null` once every step has run.
    #[napi]
    pub fn next_step(&self) -> Result<Option<f64>> {
        self.now(|txn| {
            Ok(txn
                .migration()?
                .next_step()
                .map_err(to_js_error)?
                .map(u64_number))
        })
    }

    #[napi(ts_return_type = "Array<number | bigint | string | Buffer>")]
    pub fn previous_keys(&self, collection: String) -> Result<Vec<JsKeyOut>> {
        self.now(|txn| txn.previous_keys(&collection))?.deliver()
    }

    #[napi(ts_return_type = "Buffer | null")]
    pub fn previous_record<'env>(
        &self,
        env: &'env Env,
        collection: String,
        #[napi(ts_arg_type = "number | bigint | string | Uint8Array")] key: Unknown<'_>,
    ) -> Result<Option<BufferSlice<'env>>> {
        let key = key_in(key)?;

        self.now(|txn| txn.previous_record(&collection, key))?
            .map(|record| js_bytes(env, record))
            .transpose()
    }

    /// Commits a migration, and returns the open database.
    #[napi]
    pub fn finish(&self) -> Result<NativeDatabase> {
        finish(self.take()?)?.deliver()
    }

    #[napi(ts_return_type = "Promise<NativeDatabase | NativeFailure>")]
    pub fn finish_async(&self) -> Result<AsyncTask<Work<darudb::Database>>> {
        let txn = self.take()?;

        Ok(Work::task(move || finish(txn)))
    }
}

// What each operation of a batch is, as `lib/async.js` numbers them.
const OP_GET: u8 = 0;
const OP_FIND: u8 = 1;
const OP_FIND_FIRST: u8 = 2;
const OP_COUNT: u8 = 3;
const OP_INSERT: u8 = 4;
const OP_PUT: u8 = 5;
const OP_DELETE: u8 = 6;
const OP_PREVIOUS_RECORD: u8 = 7;
const OP_PREVIOUS_KEYS: u8 = 8;

/// One operation of a batch, with what it takes checked and copied, so that
/// it can move to a pool thread.
enum Op {
    Get(String, darudb::Value),
    Find(Vec<u8>, bool),
    Count(Vec<u8>),
    Write(String, Vec<u8>, bool),
    Delete(String, darudb::Value),
    PreviousRecord(String, darudb::Value),
    PreviousKeys(String),
    /// An operation refused before it runs, with the error it fails with.
    Refused(napi::Error<&'static str>),
}

impl Op {
    fn of(kind: u8, collection: String, key: Option<Unknown<'_>>, payload: Option<Buffer>) -> Self {
        let key = || {
            key.ok_or_else(|| invalid("the operation takes a key"))
                .and_then(key_in)
        };
        let payload = || {
            payload
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| invalid("the operation takes bytes"))
        };
        let op = match kind {
            OP_GET => key().map(|key| Op::Get(collection, key)),
            OP_FIND | OP_FIND_FIRST => payload().map(|ir| Op::Find(ir, kind == OP_FIND_FIRST)),
            OP_COUNT => payload().map(Op::Count),
            OP_INSERT | OP_PUT => {
                payload().map(|records| Op::Write(collection, records, kind == OP_PUT))
            }
            OP_DELETE => key().map(|key| Op::Delete(collection, key)),
            OP_PREVIOUS_RECORD => key().map(|key| Op::PreviousRecord(collection, key)),
            OP_PREVIOUS_KEYS => Ok(Op::PreviousKeys(collection)),
            _ => Err(invalid(format!("{kind} is not an operation"))),
        };

        op.unwrap_or_else(Op::Refused)
    }

    fn run(self, txn: &mut Txn) -> Result<Outcome> {
        Ok(match self {
            Op::Get(collection, key) => Outcome::Record(txn.get_record(&collection, key)?),
            Op::Find(ir, first) => Outcome::Bytes(txn.find_ir(&ir, first)?),
            Op::Count(ir) => Outcome::Number(txn.count_ir(&ir)?),
            Op::Write(collection, records, replace) => {
                Outcome::Keys(txn.write_records(&collection, &records, replace)?.0)
            }
            Op::Delete(collection, key) => Outcome::Bool(txn.delete(&collection, key)?),
            Op::PreviousRecord(collection, key) => {
                Outcome::Record(txn.previous_record(&collection, key)?)
            }
            Op::PreviousKeys(collection) => Outcome::Keys(txn.previous_keys(&collection)?.0),
            Op::Refused(error) => return Err(error),
        })
    }
}

/// What an operation of a batch returned.
enum Outcome {
    Bytes(Vec<u8>),
    Record(Option<Vec<u8>>),
    Number(f64),
    Bool(bool),
    Keys(Vec<darudb::Value>),
}

/// The results of a batch, one after another, each after a tag byte:
///
/// - `TAG_BYTES`: a varint length and the bytes.
/// - `TAG_NULL`, `TAG_FALSE` and `TAG_TRUE`: nothing more.
/// - `TAG_NUMBER`: a float, 8 bytes little-endian.
/// - `TAG_KEYS`: a varint count, then each key after its own tag:
///   `KEY_INT` and an int, 8 bytes little-endian; `KEY_STRING` or
///   `KEY_BYTES`, a varint length and the bytes.
/// - `TAG_FAILURE`: the error's code and message, each a varint length and
///   UTF-8.
///
/// One buffer for the whole batch costs one allocation and one crossing into
/// JavaScript, where a value per result would cost several calls each.
#[derive(Default)]
struct Batch(Vec<u8>);

const TAG_BYTES: u8 = 0;
const TAG_NULL: u8 = 1;
const TAG_NUMBER: u8 = 2;
const TAG_FALSE: u8 = 3;
const TAG_TRUE: u8 = 4;
const TAG_KEYS: u8 = 5;
const TAG_FAILURE: u8 = 6;

const KEY_INT: u8 = 0;
const KEY_STRING: u8 = 1;
const KEY_BYTES: u8 = 2;

impl Batch {
    fn push(&mut self, outcome: Result<Outcome>) {
        let outcome = outcome.and_then(|outcome| match outcome {
            Outcome::Keys(keys) if !keys.iter().all(is_key) => Err(invalid(
                "the engine returned a key of a type keys do not have",
            )),
            outcome => Ok(outcome),
        });

        match outcome {
            Ok(Outcome::Bytes(bytes) | Outcome::Record(Some(bytes))) => {
                self.0.push(TAG_BYTES);
                self.bytes(&bytes);
            }
            Ok(Outcome::Record(None)) => self.0.push(TAG_NULL),
            Ok(Outcome::Number(number)) => {
                self.0.push(TAG_NUMBER);
                self.0.extend_from_slice(&number.to_le_bytes());
            }
            Ok(Outcome::Bool(value)) => self.0.push(if value { TAG_TRUE } else { TAG_FALSE }),
            Ok(Outcome::Keys(keys)) => {
                self.0.push(TAG_KEYS);
                push_varint(&mut self.0, keys.len());

                for key in keys {
                    match key {
                        darudb::Value::Int(int) => {
                            self.0.push(KEY_INT);
                            self.0.extend_from_slice(&int.to_le_bytes());
                        }
                        darudb::Value::String(string) => {
                            self.0.push(KEY_STRING);
                            self.bytes(string.as_bytes());
                        }
                        darudb::Value::Bytes(bytes) => {
                            self.0.push(KEY_BYTES);
                            self.bytes(&bytes);
                        }
                        // Checked above.
                        _ => {}
                    }
                }
            }
            Err(error) => {
                self.0.push(TAG_FAILURE);
                self.bytes(error.status.as_bytes());
                self.bytes(error.reason.as_bytes());
            }
        }
    }

    fn bytes(&mut self, bytes: &[u8]) {
        push_varint(&mut self.0, bytes.len());
        self.0.extend_from_slice(bytes);
    }
}

fn is_key(value: &darudb::Value) -> bool {
    matches!(
        value,
        darudb::Value::Int(_) | darudb::Value::String(_) | darudb::Value::Bytes(_)
    )
}

fn commit(txn: Txn, deferred: bool) -> Result<()> {
    match txn {
        Txn::Write(txn) if deferred => txn.commit_deferred(),
        Txn::Write(txn) => txn.commit(),
        _ => return Err(invalid("only a write transaction commits")),
    }
    .map_err(to_js_error)
}

fn finish(txn: Txn) -> Result<darudb::Database> {
    match txn {
        Txn::Migration(pending) => pending.finish().map_err(to_js_error),
        _ => Err(invalid("only a migration finishes")),
    }
}

/// The name of a collection, made once for the calls that read and write its
/// objects: turning the JavaScript string into one of Rust's on every call
/// cost a twentieth of a `get`.
#[napi(ts_return_type = "ExternalObject<'CollectionName'>")]
pub fn collection_name(name: String) -> External<String> {
    External::new(name)
}

/// The record of the object whose key is `key`, delivered as [`handed`]
/// says, or `null`.
#[napi(ts_return_type = "number | Buffer | null")]
pub fn get_record<'env>(
    env: &'env Env,
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    #[napi(ts_arg_type = "ExternalObject<'CollectionName'>")] collection: &External<String>,
    #[napi(ts_arg_type = "number | bigint | string | Uint8Array")] key: Unknown<'_>,
    mut scratch: BufferSlice<'_>,
) -> Result<Option<Either<u32, BufferSlice<'env>>>> {
    let key = key_in(key)?;
    let mut copied = None;

    with(txn, |txn| {
        txn.get_record_with(collection, key, &mut |record| {
            copied = Some(copy_lent(record, &mut scratch));

            Ok(())
        })
    })?;

    copied.map(|copied| handed(env, copied)).transpose()
}

/// The records a query finds, one after another, each after its length;
/// only the first with `first`. Delivered as [`handed`] says.
#[napi(ts_return_type = "number | Buffer")]
pub fn find<'env>(
    env: &'env Env,
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    ir: BufferSlice<'_>,
    first: bool,
    mut scratch: BufferSlice<'_>,
) -> Result<Either<u32, BufferSlice<'env>>> {
    with(txn, |txn| txn.find_ir_into(env, &ir, first, &mut scratch))
}

#[napi]
pub fn count(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    ir: BufferSlice<'_>,
) -> Result<f64> {
    with(txn, |txn| txn.count_ir(&ir))
}

/// The records a prepared query finds with `parameters`, encoded as
/// `Query::bind_encoded` reads them, as `find` delivers them.
#[napi(ts_return_type = "number | Buffer")]
pub fn find_prepared<'env>(
    env: &'env Env,
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    #[napi(ts_arg_type = "ExternalObject<'NativePrepared'>")] prepared: &External<
        Arc<PreparedQuery>,
    >,
    parameters: BufferSlice<'_>,
    first: bool,
    mut scratch: BufferSlice<'_>,
) -> Result<Either<u32, BufferSlice<'env>>> {
    let query = prepared.bound(&parameters, first)?;

    with(txn, |txn| {
        txn.find_into(env, &prepared.collection, &query, &mut scratch)
    })
}

#[napi]
pub fn count_prepared(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    #[napi(ts_arg_type = "ExternalObject<'NativePrepared'>")] prepared: &External<
        Arc<PreparedQuery>,
    >,
    parameters: BufferSlice<'_>,
) -> Result<f64> {
    let query = prepared.bound(&parameters, false)?;

    with(txn, |txn| txn.count(&prepared.collection, &query))
}

#[napi(ts_return_type = "Array<number | bigint | string | Buffer>")]
pub fn write_records(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    #[napi(ts_arg_type = "ExternalObject<'CollectionName'>")] collection: &External<String>,
    records: BufferSlice<'_>,
    replace: bool,
) -> Result<Vec<JsKeyOut>> {
    with(txn, |txn| txn.write_records(collection, &records, replace))?.deliver()
}

/// Writes the record of one object, as `writeRecords` writes a batch, and
/// returns its key rather than an array of one: making the array cost more
/// than a twentieth of an `insert`.
#[napi(ts_return_type = "number | bigint | string | Buffer")]
pub fn write_record(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    #[napi(ts_arg_type = "ExternalObject<'CollectionName'>")] collection: &External<String>,
    records: BufferSlice<'_>,
    replace: bool,
) -> Result<JsKeyOut> {
    let Keys(keys) = with(txn, |txn| txn.write_records(collection, &records, replace))?;
    let [key] = <[darudb::Value; 1]>::try_from(keys)
        .map_err(|_| invalid("`writeRecord` takes exactly one record"))?;

    key_out(key)
}

/// Commits the write transaction `txn`, deferred or not, and ends it.
#[napi]
pub fn commit_transaction(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    deferred: bool,
) -> Result<()> {
    let taken = lock(txn)?.take().ok_or_else(ended)?;

    commit(taken, deferred)
}

/// Ends the transaction `txn`, throwing a write's changes away. Ending one
/// that has ended does nothing.
#[napi]
pub fn end_transaction(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
) {
    if let Ok(mut held) = lock(txn) {
        *held = None;
    }
}

/// Deletes the object whose key is `key`, and says whether there was one.
#[napi]
pub fn delete_object(
    #[napi(ts_arg_type = "ExternalObject<'NativeTransaction'>")] txn: &External<Held>,
    #[napi(ts_arg_type = "ExternalObject<'CollectionName'>")] collection: &External<String>,
    #[napi(ts_arg_type = "number | bigint | string | Uint8Array")] key: Unknown<'_>,
) -> Result<bool> {
    let key = key_in(key)?;

    with(txn, |txn| txn.delete(collection, key))
}

fn lock(held: &Mutex<Option<Txn>>) -> Result<MutexGuard<'_, Option<Txn>>> {
    held.lock()
        .map_err(|_| napi::Error::new("INTERNAL", "a transaction's lock was poisoned".to_owned()))
}

/// Runs `operation` on the transaction `held` holds, if it has not ended.
fn with<T>(held: &Mutex<Option<Txn>>, operation: impl FnOnce(&mut Txn) -> Result<T>) -> Result<T> {
    operation(lock(held)?.as_mut().ok_or_else(ended)?)
}

/// Parses a query in the query language into IR, with its parameters,
/// encoded as `Query::bind_encoded` reads them.
#[napi]
pub fn parse_query(
    collection: String,
    text: String,
    parameters: BufferSlice<'_>,
    count: bool,
) -> Result<Buffer> {
    let request = darudb::QueryRequest {
        collection,
        query: darudb::Query::prepare(&text)
            .and_then(|query| query.bind_encoded(&parameters))
            .map_err(to_js_error)?,
        count,
    };

    Ok(request.encode().map_err(to_js_error)?.into())
}

/// A query parsed once, whose parameters are given values each time it runs.
/// It holds no database, so one runs in any transaction.
#[napi]
pub struct NativePrepared {
    inner: Arc<PreparedQuery>,
}

/// What a prepared query holds, and shares with its handle.
pub struct PreparedQuery {
    collection: String,
    query: darudb::Query,
    /// The query cut to its first object, kept apart so that each run binds
    /// a query it shares rather than cutting a copy of it.
    first: darudb::Query,
}

#[napi]
impl NativePrepared {
    /// Prepares the query in `ir`, whose parameters stay parameters.
    #[napi(factory)]
    pub fn from_ir(ir: BufferSlice<'_>) -> Result<Self> {
        let request = darudb::QueryRequest::decode(&ir).map_err(to_js_error)?;

        Ok(Self::new(request.collection, request.query))
    }

    /// Prepares `text` in the query language, on `collection`.
    #[napi(factory)]
    pub fn from_text(collection: String, text: String) -> Result<Self> {
        Ok(Self::new(
            collection,
            darudb::Query::prepare(&text).map_err(to_js_error)?,
        ))
    }

    /// The query as `findPrepared` and `countPrepared` take it; see
    /// [`NativeTransaction::handle`].
    #[napi(getter, ts_return_type = "ExternalObject<'NativePrepared'>")]
    pub fn handle(&self) -> External<Arc<PreparedQuery>> {
        External::new(Arc::clone(&self.inner))
    }

    /// The IR of the query with `parameters` for its parameters, for the
    /// asynchronous API, which passes queries to the thread pool as IR.
    #[napi]
    pub fn bind(&self, parameters: BufferSlice<'_>, count: bool) -> Result<Buffer> {
        let request = darudb::QueryRequest {
            collection: self.inner.collection.clone(),
            query: self.inner.bound(&parameters, false)?,
            count,
        };

        Ok(request.encode().map_err(to_js_error)?.into())
    }

    fn new(collection: String, query: darudb::Query) -> Self {
        Self {
            inner: Arc::new(PreparedQuery {
                collection,
                first: query.clone().first(),
                query,
            }),
        }
    }
}

impl PreparedQuery {
    /// The query with `parameters`, encoded as `Query::bind_encoded` reads
    /// them, for its parameters, cut to its first object with `first`.
    fn bound(&self, parameters: &[u8], first: bool) -> Result<darudb::Query> {
        let query = if first { &self.first } else { &self.query };

        query.bind_encoded(parameters).map_err(to_js_error)
    }
}

/// The query in `ir`, cut to its first object with `first`.
fn request_of(ir: &[u8], first: bool) -> Result<darudb::QueryRequest> {
    let mut request = darudb::QueryRequest::decode(ir).map_err(to_js_error)?;

    if first {
        request.query = request.query.first();
    }

    Ok(request)
}

/// Appends `value` as a varint: seven bits a byte, low bits first.
fn push_varint(out: &mut Vec<u8>, mut value: usize) {
    while value >= 0x80 {
        out.push(u8::try_from(value & 0x7F).unwrap_or(0) | 0x80);
        value >>= 7;
    }

    out.push(u8::try_from(value).unwrap_or(0));
}

/// The varint at the start of `bytes`, and how many bytes it took.
fn varint(bytes: &[u8]) -> Result<(usize, usize)> {
    let mut value = 0usize;

    for (index, byte) in bytes.iter().enumerate().take(9) {
        value |= usize::from(byte & 0x7F) << (7 * index);

        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }

    Err(invalid(
        "a batch of records holds a length that does not end",
    ))
}

fn migration_of(spec: NativeMigration) -> Result<darudb::Migration> {
    let mut migration = darudb::Migration::to(u64::from(spec.version));

    for pair in spec.rename_collections {
        let [from, to] = <[String; 2]>::try_from(pair)
            .map_err(|_| invalid("a collection rename is a pair of names"))?;

        migration = migration.rename_collection(from, to);
    }

    for triple in spec.rename_fields {
        let [collection, from, to] = <[String; 3]>::try_from(triple)
            .map_err(|_| invalid("a field rename is a collection and two names"))?;

        migration = migration.rename_field(collection, from, to);
    }

    for collection in spec.delete_collections {
        migration = migration.delete_collection(collection);
    }

    for pair in spec.replace_fields {
        let [collection, field] = <[String; 2]>::try_from(pair)
            .map_err(|_| invalid("a replaced field is a collection and a field"))?;

        migration = migration.replace_field(collection, field);
    }

    Ok(migration)
}

/// A primary key as JavaScript passes it: a number, a `bigint`, a string, or
/// bytes.
///
/// The key is taken as it comes and converted by its type, which one call
/// tells. An `Either4` makes an error with a message and drops it on every
/// conversion, before it tries the first type, and that took a fiftieth of
/// a lookup by key.
fn key_in(key: Unknown<'_>) -> Result<darudb::Value> {
    const NOT_A_KEY: &str = "a primary key is a number, a bigint, a string or bytes";
    let not_a_key = |_: napi::Error| invalid(NOT_A_KEY);

    match key.get_type().map_err(not_a_key)? {
        ValueType::Number => {
            let number = f64::from_unknown(key).map_err(not_a_key)?;

            if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0 {
                Ok(number_value(number))
            } else {
                Err(invalid(format!("{number} is not an integer key")))
            }
        }
        ValueType::BigInt => bigint_value(&BigInt::from_unknown(key).map_err(not_a_key)?),
        ValueType::String => Ok(darudb::Value::String(
            String::from_unknown(key).map_err(not_a_key)?,
        )),
        ValueType::Object => Ok(darudb::Value::Bytes(
            Uint8Array::from_unknown(key).map_err(not_a_key)?.to_vec(),
        )),
        _ => Err(invalid(NOT_A_KEY)),
    }
}

/// A JavaScript number as a value: an int when it is a whole number a
/// number holds exactly, a float otherwise.
fn number_value(number: f64) -> darudb::Value {
    if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0 {
        // Checked just above to be a whole number within 2^53.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number within 2^53 converts exactly"
        )]
        let int = number as i64;

        darudb::Value::Int(int)
    } else {
        darudb::Value::Float(number)
    }
}

fn bigint_value(bigint: &BigInt) -> Result<darudb::Value> {
    match bigint.get_i64() {
        (value, true) => Ok(darudb::Value::Int(value)),
        _ => Err(invalid("a bigint beyond 64 bits")),
    }
}

fn key_out(key: darudb::Value) -> Result<JsKeyOut> {
    Ok(match key {
        darudb::Value::Int(int) if (-SAFE_INTEGER..=SAFE_INTEGER).contains(&int) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "an int within 2^53 converts exactly"
            )]
            let number = int as f64;

            Either4::A(number)
        }
        darudb::Value::Int(int) => Either4::B(BigInt::from(int)),
        darudb::Value::String(string) => Either4::C(string),
        darudb::Value::Bytes(bytes) => Either4::D(bytes.into()),
        other => return Err(invalid(format!("{other:?} is not a key"))),
    })
}

thread_local! {
    /// The records of the last query this thread ran synchronously, kept
    /// for the next to be gathered in ([`Txn::find_into`]).
    static FOUND: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Up to this many bytes, a result is copied into a buffer JavaScript owns;
/// beyond, the allocation is handed over. Handing it over saves the copy but
/// costs more than copying a record or two: V8 registers the allocation, and
/// a finalizer frees it.
const COPY_LIMIT: usize = 1 << 20;

/// Bytes the engine lends, copied once for [`handed`] to deliver: into
/// `scratch` when they fit, their length then, and into a vector of their
/// own when they do not.
fn copy_lent(bytes: &[u8], scratch: &mut [u8]) -> Either<u32, Vec<u8>> {
    match (scratch.get_mut(..bytes.len()), u32::try_from(bytes.len())) {
        (Some(room), Ok(len)) => {
            room.copy_from_slice(bytes);

            Either::A(len)
        }
        _ => Either::B(bytes.to_vec()),
    }
}

/// What [`copy_lent`] copied, as a synchronous read hands it JavaScript:
/// the length of the bytes copied into `scratch`, a buffer the JavaScript
/// side keeps for the purpose and reads before its next call, or a `Buffer`
/// of their own when they did not fit. A new `Buffer` for every read cost
/// JavaScript an allocation and a collection each time.
fn handed(env: &Env, copied: Either<u32, Vec<u8>>) -> Result<Either<u32, BufferSlice<'_>>> {
    match copied {
        Either::A(len) => Ok(Either::A(len)),
        Either::B(bytes) => js_bytes(env, bytes).map(Either::B),
    }
}

/// `bytes` as a JavaScript `Buffer`.
fn js_bytes(env: &Env, bytes: Vec<u8>) -> Result<BufferSlice<'_>> {
    if bytes.len() <= COPY_LIMIT {
        BufferSlice::copy_from(env, &bytes)
    } else {
        BufferSlice::from_data(env, bytes)
    }
    .map_err(|error| napi::Error::new("INTERNAL", error.reason.clone()))
}

fn u64_number(value: u64) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts and versions stay far below 2^53"
    )]
    let number = value as f64;

    number
}

/// The engine's error as a JavaScript one: the message as the message, and the
/// stable code as `code`.
fn to_js_error(error: darudb::Error) -> napi::Error<&'static str> {
    napi::Error::new(error.code(), error.to_string())
}

fn invalid(message: impl Into<String>) -> napi::Error<&'static str> {
    napi::Error::new("INVALID_ARGUMENT", message.into())
}

/// The error for a transaction used after it ended.
fn ended() -> napi::Error<&'static str> {
    napi::Error::new("CLOSED", "the transaction has ended".to_owned())
}
