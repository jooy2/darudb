//! Opening a file, the database handle, and the tools.
//!
//! [`open`] returns either a [`NativeDatabase`] or, for a file whose schema
//! is older than the declared one, the [`NativeTransaction`] of the
//! migration under way, which the Python code runs its migration functions
//! in, one version step at a time, before it finishes the migration into the
//! database. Reports come back as `dict`s, which `python/darudb/_database.py`
//! makes into its dataclasses.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use darudb::{BackupOptions, CheckReport, Database, Field, OpenOptions, Opening, Schema, Type};
use pyo3::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use zeroize::Zeroizing;

use crate::schema::{MigrationSpec, NativeSchema, migration_of};
use crate::transaction::{NativeTransaction, Txn};
use crate::values::{bytes_of, is_bytes, to_python};
use crate::{OrRaise, failure, invalid};

/// An open database, until it is closed.
#[pyclass(frozen, module = "darudb._native")]
pub(crate) struct NativeDatabase {
    db: Mutex<Option<Database>>,
    /// The schema the database works with, in the form a declaration has,
    /// made once as it opens: the transactions write a collection reached
    /// by name through it, so that each value takes its field's type.
    stored: Option<Arc<Schema>>,
}

impl NativeDatabase {
    pub(crate) fn new(db: Database) -> Self {
        let stored = db.stored_schema().map(Arc::new);

        Self {
            db: Mutex::new(Some(db)),
            stored,
        }
    }

    /// Another handle to the database, which a call works through without
    /// holding the lock. A closed database is `CLOSED`.
    fn handle(&self, py: Python<'_>) -> PyResult<Database> {
        self.db
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| failure(py, "CLOSED", "the database is closed"))
    }

    /// Runs `work` on another handle to the database, without the GIL.
    fn with<T: Send>(
        &self,
        py: Python<'_>,
        work: impl FnOnce(&Database) -> darudb::Result<T> + Send,
    ) -> PyResult<T> {
        let db = self.handle(py)?;

        py.detach(|| work(&db)).or_raise(py)
    }
}

#[pymethods]
impl NativeDatabase {
    /// Whether `close` has not been called.
    #[getter]
    fn is_open(&self) -> bool {
        self.db
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    #[getter]
    fn page_size(&self, py: Python<'_>) -> PyResult<u32> {
        Ok(self.handle(py)?.page_size())
    }

    #[getter]
    fn format_version(&self, py: Python<'_>) -> PyResult<u32> {
        Ok(self.handle(py)?.format_version())
    }

    #[getter]
    fn is_encrypted(&self, py: Python<'_>) -> PyResult<bool> {
        Ok(self.handle(py)?.is_encrypted())
    }

    /// Begins a read transaction.
    fn begin_read(&self, py: Python<'_>) -> PyResult<NativeTransaction> {
        self.with(py, |db| db.begin_read().map(|txn| Txn::Read(Box::new(txn))))
            .map(|txn| NativeTransaction::new(txn, self.stored.clone()))
    }

    /// Begins the write transaction, waiting for the one running.
    fn begin_write(&self, py: Python<'_>) -> PyResult<NativeTransaction> {
        self.with(py, |db| {
            db.begin_write().map(|txn| Txn::Write(Box::new(txn)))
        })
        .map(|txn| NativeTransaction::new(txn, self.stored.clone()))
    }

    /// The schema the database works with, the declared one or the file's
    /// own, as nested `dict`s, which `_schema.py` makes into its
    /// dataclasses; `None` when there is neither.
    #[getter]
    fn stored_schema<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        self.handle(py)?;

        self.stored
            .as_deref()
            .map(|schema| schema_dict(py, schema))
            .transpose()
    }

    /// Raises the engine's error for reaching a collection of a database
    /// without a schema: the file holds none, the one it holds does not
    /// read, or it stored one after the database opened. Does nothing for a
    /// database with a schema.
    fn without_schema(&self, py: Python<'_>) -> PyResult<()> {
        self.with(py, |db| {
            if db.schema_record().is_some() {
                return Ok(());
            }

            // Any name: a transaction without a schema fails before it looks
            // the collection up.
            db.begin_read()?.collection("").map(|_| ())
        })
    }

    /// Makes every commit durable.
    fn sync(&self, py: Python<'_>) -> PyResult<()> {
        self.with(py, Database::sync)
    }

    /// Checks the published commit completely.
    fn check<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let report = self.with(py, Database::check)?;

        check_report(py, &report)
    }

    /// Raises the file's format version to the newest, and says whether it
    /// did.
    fn upgrade_format(&self, py: Python<'_>) -> PyResult<bool> {
        self.with(py, Database::upgrade_format)
    }

    /// Makes the file smaller in place.
    fn compact<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let report = self.with(py, Database::compact)?;
        let dict = PyDict::new(py);

        dict.set_item("bytes_before", report.bytes_before)?;
        dict.set_item("bytes_after", report.bytes_after)?;
        dict.set_item("pages_moved", report.pages_moved)?;

        Ok(dict)
    }

    /// Writes a copy of the published commit to a new file at `path`, under
    /// a new key or password when one is given, or with no encryption when
    /// `plain` is set.
    #[pyo3(signature = (path, key = None, password = None, password_hashing = None, plain = false))]
    fn backup<'py>(
        &self,
        py: Python<'py>,
        path: PathBuf,
        key: Option<&Bound<'py, PyAny>>,
        password: Option<&Bound<'py, PyAny>>,
        password_hashing: Option<(u32, u32, u32)>,
        plain: bool,
    ) -> PyResult<Bound<'py, PyDict>> {
        one_secret(py, key, password)?;

        if plain && (key.is_some() || password.is_some()) {
            return Err(invalid(
                py,
                "give plain, a key or a password, not more than one",
            ));
        }

        let mut options = BackupOptions::new();

        if plain {
            options.plain();
        }

        if let Some(key) = key {
            options.key(*Zeroizing::new(key_of(key)?));
        }

        if let Some(password) = password {
            options.password(&*secret_of(password)?);
        }

        if let Some((memory, iterations, parallelism)) = password_hashing {
            options.password_hashing(memory, iterations, parallelism);
        }

        let report = self.with(py, |db| db.backup_with(&path, &options))?;
        let dict = PyDict::new(py);

        dict.set_item("commit_id", report.commit_id)?;
        dict.set_item("trees", report.trees)?;
        dict.set_item("entries", report.entries)?;
        dict.set_item("bytes", report.bytes)?;

        Ok(dict)
    }

    /// Changes the key of an encrypted database.
    fn set_key(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<()> {
        let key = Zeroizing::new(key_of(key)?);

        self.with(py, |db| db.set_key(*key))
    }

    /// Changes the key of an encrypted database to one derived from
    /// `password`.
    fn set_password(&self, py: Python<'_>, password: &Bound<'_, PyAny>) -> PyResult<()> {
        let password = secret_of(password)?;

        self.with(py, |db| db.set_password(&*password))
    }

    /// Makes deferred commits durable and closes this handle. Closing one
    /// that is closed does nothing.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let db = self
            .db
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        match db {
            Some(db) => py.detach(move || db.close()).or_raise(py),
            None => Ok(()),
        }
    }
}

/// Refuses a key and a password together, which would leave the caller
/// unsure which of the two opens the file.
fn one_secret(
    py: Python<'_>,
    key: Option<&Bound<'_, PyAny>>,
    password: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    if key.is_some() && password.is_some() {
        return Err(invalid(py, "give a key or a password, not both"));
    }

    Ok(())
}

/// A key of 32 bytes.
fn key_of(value: &Bound<'_, PyAny>) -> PyResult<[u8; 32]> {
    let py = value.py();

    if !is_bytes(value) {
        return Err(invalid(py, "a key is 32 bytes"));
    }

    let bytes = Zeroizing::new(bytes_of(value)?);

    bytes
        .as_slice()
        .try_into()
        .map_err(|_| invalid(py, format!("a key is 32 bytes, not {}", bytes.len())))
}

/// A password, as text or bytes.
fn secret_of(value: &Bound<'_, PyAny>) -> PyResult<Zeroizing<Vec<u8>>> {
    if let Ok(text) = value.cast::<PyString>() {
        Ok(Zeroizing::new(text.to_str()?.as_bytes().to_vec()))
    } else if is_bytes(value) {
        Ok(Zeroizing::new(bytes_of(value)?))
    } else {
        Err(invalid(value.py(), "a password is a str or bytes"))
    }
}

/// A timeout in seconds.
fn timeout_of(py: Python<'_>, seconds: f64) -> PyResult<Duration> {
    Duration::try_from_secs_f64(seconds).map_err(|_| {
        invalid(
            py,
            format!("a timeout is a number of seconds, not {seconds}"),
        )
    })
}

fn check_report<'py>(py: Python<'py>, report: &CheckReport) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let problems = PyList::empty(py);

    for problem in &report.problems {
        let entry = PyDict::new(py);

        entry.set_item("page", problem.page)?;
        entry.set_item("tree", problem.tree.as_deref())?;
        entry.set_item("message", &problem.message)?;
        problems.append(entry)?;
    }

    dict.set_item("ok", report.is_ok())?;
    dict.set_item("commit_id", report.commit_id)?;
    dict.set_item("page_count", report.page_count)?;
    dict.set_item("pages_checked", report.pages_checked)?;
    dict.set_item("objects_checked", report.objects_checked)?;
    dict.set_item("problems", problems)?;

    Ok(dict)
}

/// Opens the database at `path`. Returns the database, or the transaction
/// of the migration under way when the file holds an older schema.
#[pyfunction]
#[pyo3(signature = (
    path,
    *,
    create = None,
    page_size = None,
    busy_timeout = None,
    cache_size = None,
    schema = None,
    migrations = Vec::new(),
    key = None,
    password = None,
    password_hashing = None,
    upgrade_format = None,
))]
#[expect(
    clippy::too_many_arguments,
    reason = "the options of opening, each by name"
)]
fn open<'py>(
    py: Python<'py>,
    path: PathBuf,
    create: Option<bool>,
    page_size: Option<u32>,
    busy_timeout: Option<f64>,
    cache_size: Option<usize>,
    schema: Option<&Bound<'py, NativeSchema>>,
    migrations: Vec<MigrationSpec>,
    key: Option<&Bound<'py, PyAny>>,
    password: Option<&Bound<'py, PyAny>>,
    password_hashing: Option<(u32, u32, u32)>,
    upgrade_format: Option<bool>,
) -> PyResult<Bound<'py, PyAny>> {
    one_secret(py, key, password)?;

    let mut options = OpenOptions::new();

    if let Some(create) = create {
        options.create(create);
    }

    if let Some(page_size) = page_size {
        options.page_size(page_size);
    }

    if let Some(seconds) = busy_timeout {
        options.busy_timeout(timeout_of(py, seconds)?);
    }

    if let Some(bytes) = cache_size {
        options.cache_size(bytes);
    }

    if let Some(schema) = schema {
        options.schema(schema.get().schema.clone());
    }

    for migration in migrations {
        options.migration(migration_of(migration));
    }

    if let Some(key) = key {
        options.key(*Zeroizing::new(key_of(key)?));
    }

    if let Some(password) = password {
        options.password(&*secret_of(password)?);
    }

    if let Some((memory, iterations, parallelism)) = password_hashing {
        options.password_hashing(memory, iterations, parallelism);
    }

    if let Some(upgrade) = upgrade_format {
        options.upgrade_format(upgrade);
    }

    let opening = py
        .detach(move || options.open_migrating(path))
        .or_raise(py)?;

    match opening {
        Opening::Open(db) => NativeDatabase::new(db).into_bound_py_any(py),
        Opening::Migrating(pending) => {
            NativeTransaction::new(Txn::Migration(pending), None).into_bound_py_any(py)
        }
    }
}

/// Rescues what it can of the damaged database at `source` into a new one at
/// `target`.
#[pyfunction]
#[pyo3(signature = (source, target, *, key = None, password = None, busy_timeout = None))]
fn salvage<'py>(
    py: Python<'py>,
    source: PathBuf,
    target: PathBuf,
    key: Option<&Bound<'py, PyAny>>,
    password: Option<&Bound<'py, PyAny>>,
    busy_timeout: Option<f64>,
) -> PyResult<Bound<'py, PyDict>> {
    one_secret(py, key, password)?;

    let mut options = OpenOptions::new();

    if let Some(key) = key {
        options.key(*Zeroizing::new(key_of(key)?));
    }

    if let Some(password) = password {
        options.password(&*secret_of(password)?);
    }

    if let Some(seconds) = busy_timeout {
        options.busy_timeout(timeout_of(py, seconds)?);
    }

    let report = py
        .detach(move || options.salvage(source, target))
        .or_raise(py)?;
    let dict = PyDict::new(py);

    dict.set_item("whole", report.is_whole())?;
    dict.set_item("commit_id", report.commit_id)?;
    dict.set_item("pages_scanned", report.pages_scanned)?;
    dict.set_item("pages_damaged", report.pages_damaged)?;
    dict.set_item("pages_unread", report.pages_unread)?;
    dict.set_item("entries_recovered", report.entries_recovered)?;
    dict.set_item("values_lost", report.values_lost)?;
    dict.set_item("objects_dropped", report.objects_dropped)?;
    dict.set_item("trees", report.trees)?;
    dict.set_item("entries", report.entries)?;
    dict.set_item("bytes", report.bytes)?;

    Ok(dict)
}

/// `schema` as `NativeDatabase.stored_schema` gives it.
fn schema_dict<'py>(py: Python<'py>, schema: &Schema) -> PyResult<Bound<'py, PyDict>> {
    let collections = PyList::empty(py);

    for collection in schema.collections() {
        let indexes = PyList::empty(py);

        for declared in collection.indexes() {
            let index = PyDict::new(py);
            let fields: Vec<String> = declared.fields().collect();

            // `field` is the first, as it was when an index had one.
            index.set_item("field", fields.first())?;
            index.set_item("fields", fields)?;
            index.set_item("unique", declared.is_unique())?;
            indexes.append(index)?;
        }

        let dict = PyDict::new(py);

        dict.set_item("name", collection.name())?;
        dict.set_item("key", collection.key())?;
        dict.set_item("auto_increment", collection.is_auto_increment())?;
        dict.set_item("fields", fields_list(py, collection.fields())?)?;
        dict.set_item("indexes", indexes)?;
        collections.append(dict)?;
    }

    let dict = PyDict::new(py);

    dict.set_item("version", schema.version())?;
    dict.set_item("collections", collections)?;

    Ok(dict)
}

fn fields_list<'py>(py: Python<'py>, fields: &[Field]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);

    for field in fields {
        let dict = PyDict::new(py);

        dict.set_item("name", field.name())?;
        dict.set_item("type", type_dict(py, field.kind())?)?;
        dict.set_item("optional", field.is_optional())?;

        if let Some(default) = field.default() {
            dict.set_item("default", to_python(py, default)?)?;
        }

        list.append(dict)?;
    }

    Ok(list)
}

fn type_dict<'py>(py: Python<'py>, kind: &Type) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let name = match kind {
        Type::Bool => "bool",
        Type::Int => "int",
        Type::Float => "float",
        Type::String => "string",
        Type::Bytes => "bytes",
        Type::Date => "date",
        Type::Link(collection) => {
            dict.set_item("collection", collection)?;
            "link"
        }
        Type::List(element) => {
            dict.set_item("element", type_dict(py, element)?)?;
            "list"
        }
        Type::Object(embedded) => {
            dict.set_item("fields", fields_list(py, embedded.fields())?)?;
            "object"
        }
        _ => {
            return Err(crate::failure(
                py,
                "INTERNAL",
                format!("a field type this package does not know: {kind:?}"),
            ));
        }
    };

    dict.set_item("kind", name)?;

    Ok(dict)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeDatabase>()?;
    module.add_function(wrap_pyfunction!(open, module)?)?;
    module.add_function(wrap_pyfunction!(salvage, module)?)?;

    Ok(())
}
