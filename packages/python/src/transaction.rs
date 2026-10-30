//! Read, write and migration transactions, behind one handle.
//!
//! A [`NativeTransaction`] holds its engine transaction behind a mutex, so
//! that any thread may use it, one call at a time: the asynchronous API runs
//! each call on a thread of its pool. Every call converts its Python values
//! first, releases the GIL for the engine's work, and converts the result
//! once it has the GIL back.

use std::slice;
use std::sync::{Arc, Mutex, PoisonError};

use darudb::{
    CollectionReader, CollectionWriter, Object, PendingMigration, Query, ReadTransaction, Schema,
    Value, WriteTransaction,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::database::NativeDatabase;
use crate::query::{NativeQuery, parameters_of};
use crate::values::{Layout, dict_of, key, object_of, to_python};
use crate::{OrRaise, failure, invalid};
use darudb::__binding::Shape;

/// The engine's transaction: a read, a write, or the write transaction of a
/// migration under way.
pub(crate) enum Txn {
    Read(Box<ReadTransaction>),
    Write(Box<WriteTransaction>),
    Migration(PendingMigration),
}

/// A collection of a transaction, for reading.
enum Reader<'a> {
    Read(CollectionReader<'a>),
    Write(CollectionWriter<'a>),
}

impl Reader<'_> {
    /// The shape of `layout` in the collection.
    fn shape(&self, layout: &Layout) -> darudb::Result<Shape> {
        match self {
            Reader::Read(collection) => {
                collection.shape(layout.shape_key(), || layout.shape_fields())
            }
            Reader::Write(collection) => {
                collection.shape(layout.shape_key(), || layout.shape_fields())
            }
        }
    }

    /// Copies the record of the object with primary key `key` into
    /// `records`, and returns whether there was one.
    fn record(&self, key: Value, records: &mut Records) -> darudb::Result<bool> {
        let mut copy = |record: &[u8]| {
            records.push(record);
            Ok(())
        };

        match self {
            Reader::Read(collection) => collection.get_record_with(key, &mut copy),
            Reader::Write(collection) => collection.get_record_with(key, &mut copy),
        }
    }

    /// Copies the records of the objects `query` finds into `records`.
    fn records(&self, query: &Query, records: &mut Records) -> darudb::Result<()> {
        let copy = |record: &[u8]| {
            records.push(record);
            Ok(())
        };

        match self {
            Reader::Read(collection) => collection.query_records_with(query, copy),
            Reader::Write(collection) => collection.query_records_with(query, copy),
        }
    }

    fn count(&self, query: &Query) -> darudb::Result<u64> {
        match self {
            Reader::Read(collection) => collection.count(query),
            Reader::Write(collection) => collection.count(query),
        }
    }

    fn aggregate(
        &self,
        query: &Query,
        function: darudb::Aggregate,
        path: &[String],
    ) -> darudb::Result<Value> {
        match self {
            Reader::Read(collection) => collection.aggregate(query, function, path),
            Reader::Write(collection) => collection.aggregate(query, function, path),
        }
    }

    /// The object with primary key `key` as the engine reads it, for a
    /// collection reached by name, whose objects are `dict`s.
    fn object(&self, key: Value) -> darudb::Result<Option<Object>> {
        match self {
            Reader::Read(collection) => collection.get(key),
            Reader::Write(collection) => collection.get(key),
        }
    }

    /// The objects `query` finds as the engine reads them, as `object` does.
    fn objects(&self, query: &Query) -> darudb::Result<Vec<Object>> {
        match self {
            Reader::Read(collection) => collection.query(query),
            Reader::Write(collection) => collection.query(query),
        }
    }
}

impl Txn {
    fn reader(&mut self, collection: &str) -> darudb::Result<Reader<'_>> {
        match self {
            Txn::Read(txn) => txn.collection(collection).map(Reader::Read),
            Txn::Write(txn) => txn.collection(collection).map(Reader::Write),
            Txn::Migration(pending) => pending
                .transaction()
                .collection(collection)
                .map(Reader::Write),
        }
    }

    fn writer(&mut self, collection: &str) -> darudb::Result<CollectionWriter<'_>> {
        match self {
            Txn::Read(_) => Err(darudb::Error::InvalidArgument {
                message: "a read transaction does not write".into(),
            }),
            Txn::Write(txn) => txn.collection(collection),
            Txn::Migration(pending) => pending.transaction().collection(collection),
        }
    }

    fn migration(&mut self) -> darudb::Result<&mut PendingMigration> {
        match self {
            Txn::Migration(pending) => Ok(pending),
            _ => Err(darudb::Error::InvalidArgument {
                message: "the transaction is not a migration's".into(),
            }),
        }
    }
}

/// Records copied out of the engine one after another into one buffer, read
/// into Python objects once the GIL is back: copying each record costs less
/// than the engine's `Object` it was read into before, with an allocation
/// for each of its strings.
#[derive(Default)]
struct Records {
    bytes: Vec<u8>,
    ends: Vec<usize>,
}

impl Records {
    fn push(&mut self, record: &[u8]) {
        self.bytes.extend_from_slice(record);
        self.ends.push(self.bytes.len());
    }

    fn iter(&self) -> impl Iterator<Item = &[u8]> {
        let mut start = 0;

        self.ends.iter().map(move |&end| {
            let record = &self.bytes[start..end];

            start = end;
            record
        })
    }
}

/// A transaction, until it commits or ends.
#[pyclass(frozen, module = "darudb._native")]
pub(crate) struct NativeTransaction {
    held: Mutex<Option<Txn>>,
    /// The schema of the database the transaction is of, for writing the
    /// `dict`s of a collection reached by name; `None` for a migration's,
    /// whose collections are reached through classes.
    stored: Option<Arc<Schema>>,
}

impl NativeTransaction {
    pub(crate) fn new(txn: Txn, stored: Option<Arc<Schema>>) -> Self {
        Self {
            held: Mutex::new(Some(txn)),
            stored,
        }
    }

    /// The fields of `collection` in the database's schema, to convert a
    /// `dict` for: none for a collection the schema lacks, which the engine
    /// refuses when it is reached.
    fn fields_of(&self, collection: &str) -> &[darudb::Field] {
        self.stored
            .as_deref()
            .and_then(|schema| {
                schema
                    .collections()
                    .iter()
                    .find(|each| each.name() == collection)
            })
            .map_or(&[], darudb::Collection::fields)
    }

    /// Runs `work` on the transaction without the GIL. A transaction that
    /// has ended is `CLOSED`.
    fn with<T: Send>(
        &self,
        py: Python<'_>,
        work: impl FnOnce(&mut Txn) -> darudb::Result<T> + Send,
    ) -> PyResult<T> {
        py.detach(|| {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

            match held.as_mut() {
                Some(txn) => work(txn),
                None => Err(darudb::Error::Closed),
            }
        })
        .or_raise(py)
    }

    /// Takes the transaction out, to commit or finish it, waiting without
    /// the GIL for a call another thread is making on it.
    fn take(&self, py: Python<'_>) -> PyResult<Txn> {
        py.detach(|| {
            self.held
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        })
        .ok_or_else(|| failure(py, "CLOSED", "the transaction has ended"))
    }
}

#[pymethods]
impl NativeTransaction {
    /// Whether the transaction has not committed or ended.
    #[getter]
    fn is_open(&self) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Raises the engine's error for reaching a collection in a transaction
    /// of a database without a schema: the file holds none, the one it holds
    /// does not read, or it stored one after the database opened. Does
    /// nothing in a transaction of a database with one.
    fn without_schema(&self, py: Python<'_>) -> PyResult<()> {
        if self.stored.is_some() {
            return Ok(());
        }

        // Any name: a transaction without a schema fails before it looks the
        // collection up.
        self.with(py, |txn| txn.reader("").map(|_| ()))
    }

    /// The object of `collection` whose primary key is `key`, or `None`: an
    /// instance of `layout`'s class, or a `dict` without a layout.
    fn get<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        key_value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = key(key_value)?;
        let Some(layout) = layout.map(Bound::get) else {
            let found = self.with(py, |txn| txn.reader(collection)?.object(key))?;

            return match found {
                Some(object) => Ok(dict_of(py, &object)?.into_any()),
                None => Ok(py.None().into_bound(py)),
            };
        };
        let (shape, records) = self.with(py, |txn| {
            let reader = txn.reader(collection)?;
            let mut records = Records::default();

            reader.record(key, &mut records)?;
            Ok((reader.shape(layout)?, records))
        })?;

        match records.iter().next() {
            Some(record) => layout.read_record(py, &shape, record, &mut Vec::new()),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// The objects `query` finds, with `parameters` for its parameters.
    fn find<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        query: &NativeQuery,
        parameters: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyList>> {
        let query = query.bound(py, &parameters_of(parameters)?, false)?;
        let Some(layout) = layout.map(Bound::get) else {
            let found = self.with(py, |txn| txn.reader(collection)?.objects(&query))?;
            let list = PyList::empty(py);

            for object in &found {
                list.append(dict_of(py, object)?)?;
            }

            return Ok(list);
        };
        let (shape, records) = self.with(py, |txn| {
            let reader = txn.reader(collection)?;
            let mut records = Records::default();

            reader.records(&query, &mut records)?;
            Ok((reader.shape(layout)?, records))
        })?;
        let list = PyList::empty(py);
        let mut values = Vec::new();

        for record in records.iter() {
            list.append(layout.read_record(py, &shape, record, &mut values)?)?;
        }

        Ok(list)
    }

    /// The first object `query` finds, or `None`.
    fn find_one<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        query: &NativeQuery,
        parameters: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = query.bound(py, &parameters_of(parameters)?, true)?;
        let Some(layout) = layout.map(Bound::get) else {
            let found = self.with(py, |txn| txn.reader(collection)?.objects(&query))?;

            return match found.first() {
                Some(object) => Ok(dict_of(py, object)?.into_any()),
                None => Ok(py.None().into_bound(py)),
            };
        };
        let (shape, records) = self.with(py, |txn| {
            let reader = txn.reader(collection)?;
            let mut records = Records::default();

            reader.records(&query, &mut records)?;
            Ok((reader.shape(layout)?, records))
        })?;

        match records.iter().next() {
            Some(record) => layout.read_record(py, &shape, record, &mut Vec::new()),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// How many objects `query` finds.
    fn count(
        &self,
        py: Python<'_>,
        collection: &str,
        query: &NativeQuery,
        parameters: &Bound<'_, PyTuple>,
    ) -> PyResult<u64> {
        let query = query.bound(py, &parameters_of(parameters)?, false)?;

        self.with(py, |txn| txn.reader(collection)?.count(&query))
    }

    /// The aggregate whose code in the IR is `function` of the field at
    /// `path`, by the names the file stores, over the objects `query` finds.
    fn aggregate<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        query: &NativeQuery,
        parameters: &Bound<'_, PyTuple>,
        function: i64,
        path: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let function = darudb::Aggregate::from_code(function)
            .ok_or_else(|| invalid(py, format!("{function} is not an aggregate")))?;
        let query = query.bound(py, &parameters_of(parameters)?, false)?;
        let value = self.with(py, |txn| {
            txn.reader(collection)?.aggregate(&query, function, &path)
        })?;

        to_python(py, &value)
    }

    /// Inserts `object` and returns its primary key.
    fn insert<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        object: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let keys = self.write(
            py,
            collection,
            layout.map(Bound::get),
            slice::from_ref(object),
            false,
        )?;

        to_python(py, &keys[0])
    }

    /// Inserts `objects` and returns their keys. Every object is converted
    /// before any is inserted; an object the engine refuses stops the batch,
    /// and the ones before it stay inserted in the transaction.
    fn insert_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        objects: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        self.write_many(py, collection, layout, objects, false)
    }

    /// Inserts `object`, or replaces the object with its key, and returns
    /// its primary key.
    fn put<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        object: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let keys = self.write(
            py,
            collection,
            layout.map(Bound::get),
            slice::from_ref(object),
            true,
        )?;

        to_python(py, &keys[0])
    }

    /// `put` of each of `objects`, as `insert_many` inserts them.
    fn put_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        objects: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        self.write_many(py, collection, layout, objects, true)
    }

    /// Sets the fields `changes` has, by Python attribute, or by the names
    /// the file stores without a layout, in the object whose primary key is
    /// `key`, and says whether there was one.
    fn update(
        &self,
        py: Python<'_>,
        collection: &str,
        layout: Option<&Bound<'_, Layout>>,
        key_value: &Bound<'_, PyAny>,
        changes: &Bound<'_, PyDict>,
    ) -> PyResult<bool> {
        let key = key(key_value)?;
        let changes = match layout {
            Some(layout) => layout.get().to_changes(changes)?,
            None => object_of(changes, self.fields_of(collection))?,
        };

        self.with(py, |txn| txn.writer(collection)?.update(key, changes))
    }

    /// Deletes the object whose primary key is `key`, and says whether there
    /// was one.
    fn delete(
        &self,
        py: Python<'_>,
        collection: &str,
        key_value: &Bound<'_, PyAny>,
    ) -> PyResult<bool> {
        let key = key(key_value)?;

        self.with(py, |txn| txn.writer(collection)?.delete(key))
    }

    /// Commits a write transaction, waiting for the disk unless `deferred`.
    fn commit(&self, py: Python<'_>, deferred: bool) -> PyResult<()> {
        let txn = self.take(py)?;

        py.detach(move || match txn {
            Txn::Write(txn) if deferred => txn.commit_deferred(),
            Txn::Write(txn) => txn.commit(),
            Txn::Read(_) => Err(darudb::Error::InvalidArgument {
                message: "a read transaction has nothing to commit".into(),
            }),
            Txn::Migration(_) => Err(darudb::Error::InvalidArgument {
                message: "a migration commits when it finishes".into(),
            }),
        })
        .or_raise(py)
    }

    /// Ends the transaction: a write is aborted, a migration leaves the file
    /// as it was. Ending one that has ended does nothing.
    fn end(&self, py: Python<'_>) {
        // Aborting releases the writer lock, which is a call to the system.
        py.detach(|| {
            let txn = self
                .held
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();

            drop(txn);
        });
    }

    /// The schema version the file held before the migration.
    #[getter]
    fn previous_version(&self, py: Python<'_>) -> PyResult<u64> {
        self.with(py, |txn| Ok(txn.migration()?.previous_version()))
    }

    /// The schema version the migration leads to.
    #[getter]
    fn version(&self, py: Python<'_>) -> PyResult<u64> {
        self.with(py, |txn| Ok(txn.migration()?.version()))
    }

    /// Runs the engine's part of the next version step and returns its
    /// version, or `None` once every step has run.
    fn next_step(&self, py: Python<'_>) -> PyResult<Option<u64>> {
        self.with(py, |txn| txn.migration()?.next_step())
    }

    /// The keys of every object of `collection`, named as the schema before
    /// the migration names it.
    fn previous_keys<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
    ) -> PyResult<Bound<'py, PyList>> {
        let keys = self.with(py, |txn| {
            txn.migration()?.migrating().previous_keys(collection)
        })?;
        let list = PyList::empty(py);

        for key in &keys {
            list.append(to_python(py, key)?)?;
        }

        Ok(list)
    }

    /// The object of `collection` whose key is `key`, as the schema before
    /// the migration reads it: a `dict` by the names that schema stores.
    fn previous<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        key_value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = key(key_value)?;
        let found = self.with(py, |txn| {
            txn.migration()?.migrating().previous(collection, key)
        })?;

        match found {
            Some(object) => Ok(dict_of(py, &object)?.into_any()),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Runs the steps left, commits the migration and returns the database.
    fn finish(&self, py: Python<'_>) -> PyResult<NativeDatabase> {
        let txn = self.take(py)?;
        let database = py
            .detach(move || match txn {
                Txn::Migration(pending) => pending.finish(),
                _ => Err(darudb::Error::InvalidArgument {
                    message: "the transaction is not a migration's".into(),
                }),
            })
            .or_raise(py)?;

        Ok(NativeDatabase::new(database))
    }
}

impl NativeTransaction {
    /// Writes `objects`, instances of `layout`'s class, and returns their
    /// keys, replacing the objects with those keys when `replace`. Every
    /// object is converted before any is written.
    ///
    /// The objects of a class whose fields all hold scalars are written as
    /// records, through the shape of the class in the collection: their
    /// values are read with the GIL, and written into each record without
    /// it, with no engine `Object` between, whose strings and field names
    /// took a third of the native call of an insert. Any other object is
    /// converted into an `Object`, as is a batch with a value of the wrong
    /// kind, so that the engine's message names the field.
    fn write(
        &self,
        py: Python<'_>,
        collection: &str,
        layout: Option<&Layout>,
        objects: &[Bound<'_, PyAny>],
        replace: bool,
    ) -> PyResult<Vec<Value>> {
        let Some(layout) = layout else {
            let fields = self.fields_of(collection);
            let objects = objects
                .iter()
                .map(|object| object_of(object, fields))
                .collect::<PyResult<Vec<_>>>()?;

            return self.write_objects(py, collection, objects, replace);
        };
        let mut values = Vec::with_capacity(objects.len() * layout.len());
        let mut flat = true;

        for object in objects {
            if !layout.attributes(object, &mut values)? {
                flat = false;
                break;
            }
        }

        let mut held = Vec::with_capacity(values.len());

        if flat && layout.held(&values, &mut held)? {
            return self.with(py, |txn| {
                let mut writer = txn.writer(collection)?;

                held.chunks(layout.len())
                    .map(|fields| {
                        writer.write_shaped(
                            layout.shape_key(),
                            || layout.shape_fields(),
                            replace,
                            |slot, value| fields[slot].write(value),
                        )
                    })
                    .collect()
            });
        }

        let objects = objects
            .iter()
            .map(|object| layout.to_object(object))
            .collect::<PyResult<Vec<_>>>()?;

        self.write_objects(py, collection, objects, replace)
    }

    /// Writes `objects`, engine objects already, and returns their keys.
    fn write_objects(
        &self,
        py: Python<'_>,
        collection: &str,
        objects: Vec<Object>,
        replace: bool,
    ) -> PyResult<Vec<Value>> {
        self.with(py, |txn| {
            let mut writer = txn.writer(collection)?;

            objects
                .into_iter()
                .map(|object| {
                    if replace {
                        writer.put(object)
                    } else {
                        writer.insert(object)
                    }
                })
                .collect()
        })
    }

    fn write_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: Option<&Bound<'py, Layout>>,
        objects: &Bound<'py, PyAny>,
        replace: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let objects = objects
            .try_iter()
            .map_err(|_| {
                invalid(
                    py,
                    format!("a batch is an iterable of objects, not {objects}"),
                )
            })?
            .collect::<PyResult<Vec<_>>>()?;
        let keys = self.write(py, collection, layout.map(Bound::get), &objects, replace)?;
        let list = PyList::empty(py);

        for key in &keys {
            list.append(to_python(py, key)?)?;
        }

        Ok(list)
    }
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeTransaction>()
}
