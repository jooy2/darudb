//! Read, write and migration transactions, behind one handle.
//!
//! A [`NativeTransaction`] holds its engine transaction behind a mutex, so
//! that any thread may use it, one call at a time: the asynchronous API runs
//! each call on a thread of its pool. Every call converts its Python values
//! first, releases the GIL for the engine's work, and converts the result
//! once it has the GIL back.

use std::sync::{Mutex, PoisonError};

use darudb::{
    CollectionReader, CollectionWriter, Object, PendingMigration, Query, ReadTransaction, Value,
    WriteTransaction,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::database::NativeDatabase;
use crate::query::{NativeQuery, parameters_of};
use crate::values::{Layout, dict_of, key, to_python};
use crate::{OrRaise, failure, invalid};

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
    fn get(&self, key: Value) -> darudb::Result<Option<Object>> {
        match self {
            Reader::Read(collection) => collection.get(key),
            Reader::Write(collection) => collection.get(key),
        }
    }

    fn query(&self, query: &Query) -> darudb::Result<Vec<Object>> {
        match self {
            Reader::Read(collection) => collection.query(query),
            Reader::Write(collection) => collection.query(query),
        }
    }

    fn count(&self, query: &Query) -> darudb::Result<u64> {
        match self {
            Reader::Read(collection) => collection.count(query),
            Reader::Write(collection) => collection.count(query),
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

/// A transaction, until it commits or ends.
#[pyclass(frozen, module = "darudb._native")]
pub(crate) struct NativeTransaction {
    held: Mutex<Option<Txn>>,
}

impl NativeTransaction {
    pub(crate) fn new(txn: Txn) -> Self {
        Self {
            held: Mutex::new(Some(txn)),
        }
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

    /// The object of `collection` whose primary key is `key`, or `None`.
    fn get<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
        key_value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = key(key_value)?;
        let found = self.with(py, |txn| txn.reader(collection)?.get(key))?;

        match found {
            Some(object) => layout.get().to_instance(py, &object),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// The objects `query` finds, with `parameters` for its parameters.
    fn find<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
        query: &NativeQuery,
        parameters: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyList>> {
        let query = query.bound(py, &parameters_of(parameters)?, false)?;
        let found = self.with(py, |txn| txn.reader(collection)?.query(&query))?;
        let list = PyList::empty(py);

        for object in &found {
            list.append(layout.get().to_instance(py, object)?)?;
        }

        Ok(list)
    }

    /// The first object `query` finds, or `None`.
    fn find_one<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
        query: &NativeQuery,
        parameters: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = query.bound(py, &parameters_of(parameters)?, true)?;
        let found = self.with(py, |txn| txn.reader(collection)?.query(&query))?;

        match found.first() {
            Some(object) => layout.get().to_instance(py, object),
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

    /// Inserts `object` and returns its primary key.
    fn insert<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
        object: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let object = layout.get().to_object(object)?;
        let key = self.with(py, |txn| txn.writer(collection)?.insert(object))?;

        to_python(py, &key)
    }

    /// Inserts `objects` and returns their keys. Every object is converted
    /// before any is inserted; an object the engine refuses stops the batch,
    /// and the ones before it stay inserted in the transaction.
    fn insert_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
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
        layout: &Bound<'py, Layout>,
        object: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let object = layout.get().to_object(object)?;
        let key = self.with(py, |txn| txn.writer(collection)?.put(object))?;

        to_python(py, &key)
    }

    /// `put` of each of `objects`, as `insert_many` inserts them.
    fn put_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
        objects: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        self.write_many(py, collection, layout, objects, true)
    }

    /// Sets the fields `changes` has, by Python attribute, in the object
    /// whose primary key is `key`, and says whether there was one.
    fn update(
        &self,
        py: Python<'_>,
        collection: &str,
        layout: &Bound<'_, Layout>,
        key_value: &Bound<'_, PyAny>,
        changes: &Bound<'_, PyDict>,
    ) -> PyResult<bool> {
        let key = key(key_value)?;
        let changes = layout.get().to_changes(changes)?;

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
    fn write_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        layout: &Bound<'py, Layout>,
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
            .map(|object| layout.get().to_object(&object?))
            .collect::<PyResult<Vec<_>>>()?;
        let keys = self.with(py, |txn| {
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
                .collect::<darudb::Result<Vec<_>>>()
        })?;
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
