//! What each call does, once for the synchronous functions and the
//! asynchronous ones: each operation writes the bytes it hands out into a
//! vector the caller gives, and returns its status, so that a synchronous
//! call writes into the thread's buffer and an asynchronous one into a vector
//! it hands to Dart.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use zeroize::Zeroizing;

use crate::record::{self, Fields, Reader};
use crate::{Failure, Result, closed, invalid};

/// An open database. A read lock serves every call; closing takes the write
/// lock, so a call never finds the database gone halfway through. Shared
/// through an `Arc`, so that work on another thread keeps it alive.
pub struct Database {
    inner: RwLock<Option<darudb::Database>>,
}

impl Database {
    pub(crate) fn handle(database: darudb::Database) -> *const Self {
        Arc::into_raw(Arc::new(Self {
            inner: RwLock::new(Some(database)),
        }))
    }

    pub(crate) fn with<T>(
        &self,
        operation: impl FnOnce(&darudb::Database) -> Result<T>,
    ) -> Result<T> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);

        operation(inner.as_ref().ok_or_else(closed)?)
    }
}

/// What a transaction handle holds: a read or a write transaction, or a
/// migration under way, whose write transaction its functions use.
pub(crate) enum Txn {
    Read(Box<darudb::ReadTransaction>),
    Write(Box<darudb::WriteTransaction>),
    Migration(Box<darudb::PendingMigration>),
}

/// A transaction handle: `None` once it has committed or ended. A mutex, so
/// that work on another thread can take it; the Dart side sends a
/// transaction one call at a time, so the mutex is never contended.
pub struct Held(Mutex<Option<Txn>>);

pub(crate) fn handle_of(txn: Txn) -> *const Held {
    Arc::into_raw(Arc::new(Held(Mutex::new(Some(txn)))))
}

impl Held {
    fn lock(&self) -> MutexGuard<'_, Option<Txn>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn with<T>(&self, operation: impl FnOnce(&mut Txn) -> Result<T>) -> Result<T> {
        let mut txn = self.lock();

        operation(txn.as_mut().ok_or_else(ended)?)
    }

    /// Ends the transaction, throwing a write's changes away.
    pub(crate) fn end(&self) {
        *self.lock() = None;
    }
}

pub(crate) fn ended() -> Failure {
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

/// A query parsed once, for a collection, which each run gives its
/// parameters' values.
pub struct Prepared {
    pub(crate) collection: String,
    pub(crate) query: darudb::Query,
}

impl Prepared {
    /// The query bound to the parameters' values in the record `parameters`
    /// (`design/objects.md`, "The IR"), the first object alone with `first`.
    pub(crate) fn bound(&self, parameters: &[u8], first: bool) -> Result<darudb::Query> {
        let query = self.query.bind_encoded(parameters)?;

        Ok(if first { query.first() } else { query })
    }
}

/// The query whose IR is `ir`, the first object alone with `first`.
pub(crate) fn request(ir: &[u8], first: bool) -> Result<darudb::QueryRequest> {
    let mut request = darudb::QueryRequest::decode(ir)?;

    if first {
        request.query = request.query.first();
    }

    Ok(request)
}

/// A primary key, from a record's value at `bytes`: an `int`, a `string` or
/// `bytes`, tag first.
pub(crate) fn key(bytes: &[u8]) -> Result<darudb::Value> {
    Reader::new(bytes).value()?.key()
}

/// Appends `value` as a varint, as a record's lengths are written.
fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value.to_le_bytes()[0] | 0x80);
        value >>= 7;
    }

    out.push(value.to_le_bytes()[0]);
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

/// What opening gives: the database, or the migration under way.
pub(crate) enum Opened {
    Database(*const Database),
    Migration(*const Held),
}

pub(crate) fn open(path: &str, options: &[u8]) -> Result<Opened> {
    Ok(match open_options(options)?.open_migrating(path)? {
        darudb::Opening::Open(opened) => Opened::Database(Database::handle(opened)),
        darudb::Opening::Migrating(pending) => {
            Opened::Migration(handle_of(Txn::Migration(Box::new(pending))))
        }
    })
}

pub(crate) fn close(database: &Database) -> Result<i32> {
    let inner = database
        .inner
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .take();

    match inner {
        Some(inner) => inner.close().map(|()| 0).map_err(Failure::from),
        None => Ok(0),
    }
}

pub(crate) fn begin_read(database: &Database) -> Result<*const Held> {
    let read = database.with(|database| database.begin_read().map_err(Failure::from))?;

    Ok(handle_of(Txn::Read(Box::new(read))))
}

pub(crate) fn begin_write(database: &Database) -> Result<*const Held> {
    let write = database.with(|database| database.begin_write().map_err(Failure::from))?;

    Ok(handle_of(Txn::Write(Box::new(write))))
}

pub(crate) fn sync(database: &Database) -> Result<i32> {
    database.with(|database| database.sync().map(|()| 0).map_err(Failure::from))
}

pub(crate) fn set_key(database: &Database, key: &[u8]) -> Result<i32> {
    let key =
        Zeroizing::new(<[u8; 32]>::try_from(key).map_err(|_| invalid("a key is 32 bytes long"))?);

    database.with(|database| database.set_key(*key).map(|()| 0).map_err(Failure::from))
}

pub(crate) fn set_password(database: &Database, password: &[u8]) -> Result<i32> {
    database.with(|database| {
        database
            .set_password(password)
            .map(|()| 0)
            .map_err(Failure::from)
    })
}

pub(crate) fn commit(held: &Held, deferred: bool) -> Result<i32> {
    let taken = held.lock().take().ok_or_else(ended)?;

    match taken {
        Txn::Write(txn) if deferred => txn.commit_deferred(),
        Txn::Write(txn) => txn.commit(),
        _ => return Err(invalid("only a write transaction commits")),
    }
    .map(|()| 0)
    .map_err(Failure::from)
}

/// The record of the object of collection `name` whose key is `key`, into
/// `out`: 1 if there was one, and 0 otherwise.
pub(crate) fn get(held: &Held, name: &str, key: darudb::Value, out: &mut Vec<u8>) -> Result<i32> {
    held.with(|txn| {
        txn.reading(name, |collection| {
            collection.get_record_with(key, |record| {
                out.extend_from_slice(record);

                Ok(())
            })
        })
        .map(i32::from)
    })
}

/// The records `query` finds on collection `name`, each after its length,
/// into `out`.
pub(crate) fn find(
    held: &Held,
    name: &str,
    query: &darudb::Query,
    out: &mut Vec<u8>,
) -> Result<i32> {
    held.with(|txn| {
        txn.reading(name, |collection| {
            collection.query_records_with(query, |record| {
                push_varint(out, record.len() as u64);
                out.extend_from_slice(record);

                Ok(())
            })
        })
        .map(|()| 0)
    })
}

pub(crate) fn count(held: &Held, name: &str, query: &darudb::Query) -> Result<u64> {
    held.with(|txn| txn.reading(name, |collection| collection.count(query)))
}

/// Inserts, or with `replace` puts, the objects whose records are in
/// `records`, each after its length, and writes their keys into `out`. A
/// refused record stops the batch with its error, and the records before it
/// stay written.
pub(crate) fn write(
    held: &Held,
    name: &str,
    records: &[u8],
    replace: bool,
    out: &mut Vec<u8>,
) -> Result<i32> {
    held.with(|txn| {
        let mut writer = txn.writing()?.collection(name)?;
        let mut reader = Reader::new(records);

        while !reader.is_empty() {
            let record = reader.counted()?;
            let key = if replace {
                writer.put_record(record)
            } else {
                writer.insert_record(record)
            }?;

            write_key(out, &key)?;
        }

        Ok(0)
    })
}

pub(crate) fn update(held: &Held, name: &str, key: darudb::Value, changes: &[u8]) -> Result<i32> {
    held.with(|txn| {
        let found = txn
            .writing()?
            .collection(name)?
            .update_record(key, changes)?;

        Ok(i32::from(found))
    })
}

pub(crate) fn delete(held: &Held, name: &str, key: darudb::Value) -> Result<i32> {
    held.with(|txn| {
        let found = txn.writing()?.collection(name)?.delete(key)?;

        Ok(i32::from(found))
    })
}

pub(crate) fn previous_version(held: &Held) -> Result<u64> {
    held.with(|txn| Ok(txn.migration()?.previous_version()))
}

pub(crate) fn schema_record(held: &Held, previous: bool, out: &mut Vec<u8>) -> Result<i32> {
    held.with(|txn| {
        let migration = txn.migration()?;

        if previous {
            out.extend_from_slice(&migration.previous_schema_record());
        } else {
            out.extend_from_slice(migration.schema_record());
        }

        Ok(0)
    })
}

pub(crate) fn next_step(held: &Held) -> Result<Option<u64>> {
    held.with(|txn| txn.migration()?.next_step().map_err(Failure::from))
}

pub(crate) fn previous_record(
    held: &Held,
    name: &str,
    key: darudb::Value,
    out: &mut Vec<u8>,
) -> Result<i32> {
    held.with(|txn| {
        let record = txn.migration()?.migrating().previous_record(name, key)?;

        Ok(match record {
            Some(record) => {
                out.extend_from_slice(&record);
                1
            }
            None => 0,
        })
    })
}

pub(crate) fn previous_keys(held: &Held, name: &str, out: &mut Vec<u8>) -> Result<i32> {
    held.with(|txn| {
        let keys = txn.migration()?.migrating().previous_keys(name)?;

        for key in &keys {
            write_key(out, key)?;
        }

        Ok(0)
    })
}

pub(crate) fn finish(held: &Held) -> Result<*const Database> {
    let taken = held.lock().take().ok_or_else(ended)?;
    let Txn::Migration(pending) = taken else {
        return Err(invalid("only a migration finishes"));
    };

    Ok(Database::handle(pending.finish()?))
}

/// The integrity check, as a record: field 1 the commit id, 2 the pages of
/// the file, 3 the pages checked, 4 the objects checked, and 5 the problems,
/// each an object of the page (1), the tree (2) and the message (3), the
/// first two when known.
pub(crate) fn check(database: &Database, out: &mut Vec<u8>) -> Result<i32> {
    let report = database.with(|database| database.check().map_err(Failure::from))?;
    let mut fields = Fields::new(out);

    fields
        .int(1, report.commit_id)
        .int(2, report.page_count)
        .int(3, report.pages_checked)
        .int(4, report.objects_checked)
        .objects(5, &report.problems, |fields, problem| {
            if let Some(page) = problem.page {
                fields.int(1, page);
            }

            if let Some(tree) = &problem.tree {
                fields.string(2, tree);
            }

            fields.string(3, &problem.message);
        });
    fields.finish();

    Ok(i32::from(report.is_ok()))
}

/// A backup into `path`, and its report as a record: the commit id (1), the
/// trees (2), the entries (3) and the bytes (4) of the copy.
pub(crate) fn backup(database: &Database, path: &str, out: &mut Vec<u8>) -> Result<i32> {
    let report = database.with(|database| database.backup(path).map_err(Failure::from))?;
    let mut fields = Fields::new(out);

    fields
        .int(1, report.commit_id)
        .int(2, report.trees)
        .int(3, report.entries)
        .int(4, report.bytes);
    fields.finish();

    Ok(0)
}

/// Compaction, and its report as a record: the bytes before (1) and after
/// (2), and the pages moved (3).
pub(crate) fn compact(database: &Database, out: &mut Vec<u8>) -> Result<i32> {
    let report = database.with(|database| database.compact().map_err(Failure::from))?;
    let mut fields = Fields::new(out);

    fields
        .int(1, report.bytes_before)
        .int(2, report.bytes_after)
        .int(3, report.pages_moved);
    fields.finish();

    Ok(0)
}

/// Salvage of the file at `from` into a new file at `into`, with the busy
/// timeout, key or password of `options`, read as `Database.open`'s are, and
/// its report as a record: the commit id (1) when there was one, then the
/// pages scanned (2), damaged (3) and unread (4), the entries recovered (5),
/// the values lost (6), the objects dropped (7), and the trees (8), entries
/// (9) and bytes (10) of the new file. Status 1 when the new file holds the
/// commit whole.
pub(crate) fn salvage(from: &str, into: &str, options: &[u8], out: &mut Vec<u8>) -> Result<i32> {
    let report = open_options(options)?.salvage(from, into)?;
    let mut fields = Fields::new(out);

    if let Some(commit_id) = report.commit_id {
        fields.int(1, commit_id);
    }

    fields
        .int(2, report.pages_scanned)
        .int(3, report.pages_damaged)
        .int(4, report.pages_unread)
        .int(5, report.entries_recovered)
        .int(6, report.values_lost)
        .int(7, report.objects_dropped)
        .int(8, report.trees)
        .int(9, report.entries)
        .int(10, report.bytes);
    fields.finish();

    Ok(i32::from(report.is_whole()))
}
