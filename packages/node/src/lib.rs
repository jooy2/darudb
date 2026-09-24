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
//! field.
//!
//! An error thrown from here is a JavaScript `Error` whose `code` is the
//! engine's [`darudb::Error::code`], unchanged.

use napi::bindgen_prelude::{BigInt, Buffer, Either4, Either5, Uint8Array};
use napi_derive::napi;

/// A result whose error becomes a JavaScript `Error` with the engine's code.
///
/// It has to be named `Result`: `#[napi]` recognises a fallible function by
/// the name of its return type, and treats any other name as a value to
/// convert.
type Result<T> = napi::Result<T, &'static str>;

/// A primary key or a parameter as JavaScript passes it: a number, a
/// `bigint`, a string, or bytes.
type JsKey = Either4<f64, BigInt, String, Uint8Array>;

/// A value of a query parameter as JavaScript passes it.
type JsParameter = Either5<bool, f64, BigInt, String, Uint8Array>;

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
    /// The declared schema, as `Schema::decode` reads it.
    pub schema: Option<Buffer>,
    pub migrations: Option<Vec<NativeMigration>>,
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
        let mut open_options = darudb::OpenOptions::new();

        if let Some(create) = options.create {
            open_options.create(create);
        }

        if let Some(page_size) = options.page_size {
            open_options.page_size(page_size);
        }

        if let Some(schema) = &options.schema {
            open_options.schema(darudb::Schema::decode(schema).map_err(to_js_error)?);
        }

        for migration in options.migrations.unwrap_or_default() {
            open_options.migration(migration_of(migration)?);
        }

        let state = open_options.open_migrating(&path).map_err(to_js_error)?;

        Ok(Self { state: Some(state) })
    }

    /// Whether a migration is under way.
    #[napi(getter)]
    pub fn is_migrating(&self) -> bool {
        matches!(self.state, Some(darudb::Opening::Migrating(_)))
    }

    #[napi(getter)]
    pub fn previous_version(&self) -> Result<f64> {
        Ok(u64_number(self.pending_ref()?.previous_version()))
    }

    /// The record of the schema the migration leads to.
    #[napi(getter)]
    pub fn schema_record(&self) -> Result<Buffer> {
        Ok(self.pending_ref()?.schema_record().to_vec().into())
    }

    #[napi(getter)]
    pub fn previous_schema_record(&self) -> Result<Buffer> {
        Ok(self.pending_ref()?.previous_schema_record().into())
    }

    /// Runs the next version step's own function, and returns its version,
    /// or `null` once every step has run.
    #[napi]
    pub fn next_step(&mut self) -> Result<Option<f64>> {
        Ok(self
            .pending()?
            .next_step()
            .map_err(to_js_error)?
            .map(u64_number))
    }

    #[napi]
    pub fn previous_keys(&mut self, collection: String) -> Result<Vec<JsKeyOut>> {
        let keys = self
            .pending()?
            .migrating()
            .previous_keys(&collection)
            .map_err(to_js_error)?;

        keys.into_iter().map(key_out).collect()
    }

    #[napi]
    pub fn previous_record(&mut self, collection: String, key: JsKey) -> Result<Option<Buffer>> {
        let key = key_in(key)?;

        Ok(self
            .pending()?
            .migrating()
            .previous_record(&collection, key)
            .map_err(to_js_error)?
            .map(Buffer::from))
    }

    #[napi]
    pub fn get_record(&mut self, collection: String, key: JsKey) -> Result<Option<Buffer>> {
        get_record(self.transaction()?, &collection, key)
    }

    #[napi]
    pub fn find(&mut self, ir: Buffer) -> Result<Buffer> {
        find(self.transaction()?, &ir)
    }

    #[napi]
    pub fn count(&mut self, ir: Buffer) -> Result<f64> {
        count(self.transaction()?, &ir)
    }

    #[napi]
    pub fn write_records(
        &mut self,
        collection: String,
        records: Buffer,
        replace: bool,
    ) -> Result<Vec<JsKeyOut>> {
        write_records(self.transaction()?, &collection, &records, replace)
    }

    #[napi]
    pub fn delete(&mut self, collection: String, key: JsKey) -> Result<bool> {
        delete(self.transaction()?, &collection, key)
    }

    /// The open database, after committing a migration if one is under way.
    #[napi]
    pub fn finish(&mut self) -> Result<NativeDatabase> {
        let state = self.state.take().ok_or_else(ended)?;

        Ok(NativeDatabase {
            inner: Some(state.complete().map_err(to_js_error)?),
        })
    }

    /// Leaves the file as it was, and ends the opening.
    #[napi]
    pub fn abort(&mut self) {
        self.state = None;
    }

    fn pending_ref(&self) -> Result<&darudb::PendingMigration> {
        match &self.state {
            Some(darudb::Opening::Migrating(pending)) => Ok(pending),
            _ => Err(ended()),
        }
    }

    fn pending(&mut self) -> Result<&mut darudb::PendingMigration> {
        match &mut self.state {
            Some(darudb::Opening::Migrating(pending)) => Ok(pending),
            _ => Err(ended()),
        }
    }

    fn transaction(&mut self) -> Result<&mut darudb::WriteTransaction> {
        match &mut self.state {
            Some(darudb::Opening::Migrating(pending)) => Ok(pending.transaction()),
            _ => Err(ended()),
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
    pub fn begin_read(&self) -> Result<NativeRead> {
        Ok(NativeRead {
            inner: Some(self.database()?.begin_read().map_err(to_js_error)?),
        })
    }

    #[napi]
    pub fn begin_write(&self) -> Result<NativeWrite> {
        Ok(NativeWrite {
            inner: Some(self.database()?.begin_write().map_err(to_js_error)?),
        })
    }

    #[napi]
    pub fn sync(&self) -> Result<()> {
        self.database()?.sync().map_err(to_js_error)
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

    fn database(&self) -> Result<&darudb::Database> {
        self.inner
            .as_ref()
            .ok_or_else(|| to_js_error(darudb::Error::Closed))
    }
}

/// A read transaction.
#[napi]
pub struct NativeRead {
    inner: Option<darudb::ReadTransaction>,
}

#[napi]
impl NativeRead {
    #[napi]
    pub fn get_record(&self, collection: String, key: JsKey) -> Result<Option<Buffer>> {
        let txn = self.inner.as_ref().ok_or_else(ended)?;
        let key = key_in(key)?;

        Ok(txn
            .collection(&collection)
            .and_then(|collection| collection.get_record(key))
            .map_err(to_js_error)?
            .map(Buffer::from))
    }

    #[napi]
    pub fn find(&self, ir: Buffer) -> Result<Buffer> {
        let txn = self.inner.as_ref().ok_or_else(ended)?;
        let request = darudb::QueryRequest::decode(&ir).map_err(to_js_error)?;
        let records = txn
            .collection(&request.collection)
            .and_then(|collection| collection.query_records(&request.query))
            .map_err(to_js_error)?;

        Ok(concatenate(records).into())
    }

    #[napi]
    pub fn count(&self, ir: Buffer) -> Result<f64> {
        let txn = self.inner.as_ref().ok_or_else(ended)?;
        let request = darudb::QueryRequest::decode(&ir).map_err(to_js_error)?;

        txn.collection(&request.collection)
            .and_then(|collection| collection.count(&request.query))
            .map(u64_number)
            .map_err(to_js_error)
    }

    /// Ends the transaction. Ending one that has ended does nothing.
    #[napi]
    pub fn end(&mut self) {
        self.inner = None;
    }
}

/// A write transaction.
#[napi]
pub struct NativeWrite {
    inner: Option<darudb::WriteTransaction>,
}

#[napi]
impl NativeWrite {
    #[napi]
    pub fn get_record(&mut self, collection: String, key: JsKey) -> Result<Option<Buffer>> {
        get_record(self.transaction()?, &collection, key)
    }

    #[napi]
    pub fn find(&mut self, ir: Buffer) -> Result<Buffer> {
        find(self.transaction()?, &ir)
    }

    #[napi]
    pub fn count(&mut self, ir: Buffer) -> Result<f64> {
        count(self.transaction()?, &ir)
    }

    #[napi]
    pub fn write_records(
        &mut self,
        collection: String,
        records: Buffer,
        replace: bool,
    ) -> Result<Vec<JsKeyOut>> {
        write_records(self.transaction()?, &collection, &records, replace)
    }

    #[napi]
    pub fn delete(&mut self, collection: String, key: JsKey) -> Result<bool> {
        delete(self.transaction()?, &collection, key)
    }

    /// Commits, deferred or not, and ends the transaction.
    #[napi]
    pub fn commit(&mut self, deferred: bool) -> Result<()> {
        let txn = self.inner.take().ok_or_else(ended)?;

        if deferred {
            txn.commit_deferred()
        } else {
            txn.commit()
        }
        .map_err(to_js_error)
    }

    /// Throws the changes away and ends the transaction. Aborting one that
    /// has ended does nothing.
    #[napi]
    pub fn abort(&mut self) {
        self.inner = None;
    }

    fn transaction(&mut self) -> Result<&mut darudb::WriteTransaction> {
        self.inner.as_mut().ok_or_else(ended)
    }
}

/// Parses a query in the query language into IR, with its parameters.
#[napi]
pub fn parse_query(
    collection: String,
    text: String,
    parameters: Vec<Option<JsParameter>>,
    count: bool,
) -> Result<Buffer> {
    let parameters = parameters
        .into_iter()
        .map(|parameter| match parameter {
            None => Ok(darudb::Value::Null),
            Some(Either5::A(value)) => Ok(darudb::Value::Bool(value)),
            Some(Either5::B(value)) => Ok(number_value(value)),
            Some(Either5::C(value)) => bigint_value(&value),
            Some(Either5::D(value)) => Ok(darudb::Value::String(value)),
            Some(Either5::E(value)) => Ok(darudb::Value::Bytes(value.to_vec())),
        })
        .collect::<Result<Vec<_>>>()?;
    let request = darudb::QueryRequest {
        collection,
        query: darudb::Query::parse(&text, &parameters).map_err(to_js_error)?,
        count,
    };

    Ok(request.encode().map_err(to_js_error)?.into())
}

fn get_record(
    txn: &mut darudb::WriteTransaction,
    collection: &str,
    key: JsKey,
) -> Result<Option<Buffer>> {
    let key = key_in(key)?;

    Ok(txn
        .collection(collection)
        .and_then(|collection| collection.get_record(key))
        .map_err(to_js_error)?
        .map(Buffer::from))
}

fn find(txn: &mut darudb::WriteTransaction, ir: &[u8]) -> Result<Buffer> {
    let request = darudb::QueryRequest::decode(ir).map_err(to_js_error)?;
    let records = txn
        .collection(&request.collection)
        .and_then(|collection| collection.query_records(&request.query))
        .map_err(to_js_error)?;

    Ok(concatenate(records).into())
}

fn count(txn: &mut darudb::WriteTransaction, ir: &[u8]) -> Result<f64> {
    let request = darudb::QueryRequest::decode(ir).map_err(to_js_error)?;

    txn.collection(&request.collection)
        .and_then(|collection| collection.count(&request.query))
        .map(u64_number)
        .map_err(to_js_error)
}

/// Writes the records in `records`, each after its length, and returns their
/// keys. A refused record stops the batch there, and the records before it
/// stay written.
fn write_records(
    txn: &mut darudb::WriteTransaction,
    collection: &str,
    records: &[u8],
    replace: bool,
) -> Result<Vec<JsKeyOut>> {
    let mut writer = txn.collection(collection).map_err(to_js_error)?;
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

        keys.push(key_out(key)?);
        at = end;
    }

    Ok(keys)
}

fn delete(txn: &mut darudb::WriteTransaction, collection: &str, key: JsKey) -> Result<bool> {
    let key = key_in(key)?;

    txn.collection(collection)
        .and_then(|mut collection| collection.delete(key))
        .map_err(to_js_error)
}

/// Records one after another, each after its length as a varint.
fn concatenate(records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.iter().map(|record| record.len() + 3).sum());

    for record in records {
        let mut len = record.len();

        while len >= 0x80 {
            out.push(u8::try_from(len & 0x7F).unwrap_or(0) | 0x80);
            len >>= 7;
        }

        out.push(u8::try_from(len).unwrap_or(0));
        out.extend_from_slice(&record);
    }

    out
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

fn key_in(key: JsKey) -> Result<darudb::Value> {
    match key {
        Either4::A(number) if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0 => {
            Ok(number_value(number))
        }
        Either4::A(number) => Err(invalid(format!("{number} is not an integer key"))),
        Either4::B(bigint) => bigint_value(&bigint),
        Either4::C(string) => Ok(darudb::Value::String(string)),
        Either4::D(bytes) => Ok(darudb::Value::Bytes(bytes.to_vec())),
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

/// The error for a transaction or an opening used after it ended.
fn ended() -> napi::Error<&'static str> {
    napi::Error::new("CLOSED", "the transaction has ended".to_owned())
}
