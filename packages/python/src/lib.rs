//! The native half of the Python binding of DaruDB: the engine in
//! `crates/darudb` as the extension module `darudb._native`, which the Python
//! code in `python/darudb` wraps in the API a user sees.
//!
//! It decides nothing of its own. Objects cross as the engine's dynamic
//! [`darudb::Object`], which the engine checks against the schema by name,
//! and queries as the IR of `design/objects.md`, which the engine parses and
//! plans; this crate only converts between Python values and those.
//!
//! - `values`: Python values and objects to the engine's values, and back,
//!   through the [`Layout`](values::Layout) of each class.
//! - `schema`: the schema and migrations Python declares, as the engine's.
//! - `query`: a query Python builds, as IR, and the query language.
//! - `database`: opening a file, the database handle, and the tools.
//! - `transaction`: read, write and migration transactions.
//!
//! The rules every function keeps:
//!
//! - **The GIL is released** for everything the engine does, through
//!   `Python::detach`, so other Python threads run while a call reads the
//!   disk or waits for a writer. Python values are converted before and
//!   after, with the GIL held.
//! - **Errors** are `darudb.DaruError`, defined in `python/darudb/_errors.py`,
//!   with the engine's code unchanged. A Python value the engine cannot hold
//!   is `INVALID_ARGUMENT` too, rather than a `TypeError`, so that a caller
//!   catches one kind of error for every refusal.
//! - **Handles** are thread-safe: each holds its engine object behind a
//!   mutex, so the module declares itself safe for free-threaded Python.

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyType;

mod database;
mod query;
mod schema;
mod transaction;
mod values;

/// `darudb.DaruError`, imported the first time an error is raised.
static DARU_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();

/// A `DaruError` with `code` and `message`.
pub(crate) fn failure(py: Python<'_>, code: &str, message: impl Into<String>) -> PyErr {
    let message = message.into();

    match DARU_ERROR.get_or_try_init(py, || {
        py.import("darudb._errors")?
            .getattr("DaruError")?
            .cast_into::<PyType>()
            .map(Bound::unbind)
            .map_err(PyErr::from)
    }) {
        Ok(class) => PyErr::from_type(class.bind(py).clone(), (code.to_owned(), message)),
        Err(error) => error,
    }
}

/// The engine's error as a `DaruError`.
pub(crate) fn engine_error(py: Python<'_>, error: &darudb::Error) -> PyErr {
    failure(py, error.code(), error.to_string())
}

/// An `INVALID_ARGUMENT` error.
pub(crate) fn invalid(py: Python<'_>, message: impl Into<String>) -> PyErr {
    failure(py, "INVALID_ARGUMENT", message)
}

/// Converts an engine result, raising its error as a `DaruError`.
pub(crate) trait OrRaise<T> {
    fn or_raise(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> OrRaise<T> for darudb::Result<T> {
    fn or_raise(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|error| engine_error(py, &error))
    }
}

/// The version of the engine inside the package.
#[pyfunction]
fn engine_version() -> &'static str {
    darudb::VERSION
}

#[pymodule]
#[pyo3(name = "_native")]
fn native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(engine_version, module)?)?;
    module.add("FORMAT_VERSION", darudb::FORMAT_VERSION)?;
    values::register(module)?;
    schema::register(module)?;
    query::register(module)?;
    database::register(module)?;
    transaction::register(module)?;

    Ok(())
}
