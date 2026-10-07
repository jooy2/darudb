//! A query Python builds, as the engine's [`Query`], through the IR of
//! `design/objects.md`, and a query in the query language.
//!
//! `python/darudb/_query.py` hands this module a filter as nested tuples,
//! which [`compile_query`] writes as IR and the engine reads with
//! [`QueryRequest::decode`], so that a query built in Python means what the
//! same query means in every other language, parameters included. A query is
//! compiled once per collection and kept, and each run binds its parameters.
//!
//! The tuples, by their first element:
//!
//! - `(0, op, path, values)`: a test of the field at `path`, a tuple of
//!   names, with the operator's code of `design/objects.md` and its values.
//! - `(1, terms)` and `(2, terms)`: `AND` and `OR` of a tuple of filters.
//! - `(3, term)`: `NOT` of a filter.

use darudb::{Query, QueryRequest, Value};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyFloat, PyInt, PyString, PyTuple};

use crate::values::{bytes_of, is_bytes, plain};
use crate::{OrRaise, failure};

/// The record tags of `design/objects.md` the IR uses.
const FALSE: u8 = 0x02;
const TRUE: u8 = 0x03;
const INT: u8 = 0x04;
const FLOAT: u8 = 0x05;
const STRING: u8 = 0x06;
const BYTES: u8 = 0x07;
const LIST: u8 = 0x08;
const OBJECT: u8 = 0x09;

/// How deeply a filter may nest, as the engine holds every query to.
const MAX_DEPTH: usize = 24;

/// A parameter in place of a value, in a query that is prepared: each run
/// gives its value. `Param(0)` is the first.
#[pyclass(frozen, eq, hash, module = "darudb._native")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub(crate) struct Param {
    /// The parameter's number, from 0.
    #[pyo3(get)]
    index: u32,
}

#[pymethods]
impl Param {
    #[new]
    fn new(index: u32) -> Self {
        Self { index }
    }

    fn __repr__(&self) -> String {
        format!("param({})", self.index)
    }
}

/// A query the engine has read, on one collection.
#[pyclass(frozen, module = "darudb._native")]
#[derive(Debug)]
pub(crate) struct NativeQuery {
    query: Query,
}

impl NativeQuery {
    /// The query with `parameters` in place of its parameters, or as it is
    /// if it has none.
    pub(crate) fn bound(
        &self,
        py: Python<'_>,
        parameters: &[Value],
        first: bool,
    ) -> PyResult<Query> {
        let query = self.query.bind(parameters).or_raise(py)?;

        Ok(if first { query.first() } else { query })
    }
}

/// The values of a query's parameters.
pub(crate) fn parameters_of(parameters: &Bound<'_, PyTuple>) -> PyResult<Vec<Value>> {
    parameters.iter().map(|value| query_value(&value)).collect()
}

/// A value a query compares with.
fn query_value(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    plain(value).map_err(|_| {
        failure(
            value.py(),
            "INVALID_QUERY",
            format!("a query compares with single values, not {value}"),
        )
    })
}

/// The query on `collection` of `filter`, `sort`, `offset` and `limit`.
#[pyfunction]
#[pyo3(signature = (collection, filter, sort, offset, limit))]
fn compile_query(
    py: Python<'_>,
    collection: &str,
    filter: Option<Bound<'_, PyAny>>,
    sort: Vec<(Vec<String>, bool)>,
    offset: u64,
    limit: Option<u64>,
) -> PyResult<NativeQuery> {
    let mut entries: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut value = Vec::new();

    value.push(STRING);
    string(&mut value, collection);
    entries.push((1, value));

    if let Some(filter) = filter {
        let mut value = Vec::new();

        expression(&mut value, &filter, 1).map_err(|error| match error {
            Fault::Python(error) => error,
            Fault::Query(message) => failure(py, "INVALID_QUERY", message),
        })?;
        entries.push((2, value));
    }

    if !sort.is_empty() {
        let mut value = vec![LIST];

        varint(&mut value, sort.len() as u64);

        for (path, descending) in &sort {
            let mut record = Vec::new();

            varint(&mut record, 2);
            varint(&mut record, 1);
            write_path(&mut record, path);
            varint(&mut record, 2);
            record.push(if *descending { TRUE } else { FALSE });
            embed(&mut value, &record);
        }

        entries.push((3, value));
    }

    if offset > 0 {
        let mut value = vec![INT];

        int(&mut value, i64::try_from(offset).unwrap_or(i64::MAX));
        entries.push((4, value));
    }

    if let Some(limit) = limit {
        let mut value = vec![INT];

        int(&mut value, i64::try_from(limit).unwrap_or(i64::MAX));
        entries.push((5, value));
    }

    let mut ir = Vec::new();

    varint(&mut ir, entries.len() as u64);

    for (id, value) in entries {
        varint(&mut ir, id);
        ir.extend_from_slice(&value);
    }

    QueryRequest::decode(&ir)
        .map(|request| NativeQuery {
            query: request.query,
        })
        .or_raise(py)
}

/// A query in the query language, with `$0`, `$1` and on for its parameters.
#[pyfunction]
fn parse_query(py: Python<'_>, text: &str) -> PyResult<NativeQuery> {
    Query::prepare(text)
        .map(|query| NativeQuery { query })
        .or_raise(py)
}

/// What can go wrong writing a filter: a Python error reading it, or a
/// filter the IR cannot hold.
enum Fault {
    Python(PyErr),
    Query(String),
}

impl From<PyErr> for Fault {
    fn from(error: PyErr) -> Self {
        Self::Python(error)
    }
}

/// Writes the filter `node` as an embedded object.
fn expression(out: &mut Vec<u8>, node: &Bound<'_, PyAny>, depth: usize) -> Result<(), Fault> {
    if depth > MAX_DEPTH {
        return Err(Fault::Query(format!(
            "the filter nests more than {MAX_DEPTH} levels deep"
        )));
    }

    let node = node
        .cast::<PyTuple>()
        .map_err(|_| Fault::Query("a filter holds something that is not a condition".into()))?;
    let kind: u8 = node.get_item(0)?.extract()?;
    let mut record = Vec::new();

    match kind {
        0 => {
            let op: i64 = node.get_item(1)?.extract()?;
            let path: Vec<String> = node.get_item(2)?.extract()?;
            let values = node
                .get_item(3)?
                .cast_into::<PyTuple>()
                .map_err(PyErr::from)?;

            varint(&mut record, if values.is_empty() { 2 } else { 3 });
            varint(&mut record, 1);
            record.push(INT);
            int(&mut record, op);
            varint(&mut record, 2);
            write_path(&mut record, &path);

            if !values.is_empty() {
                varint(&mut record, 3);
                record.push(LIST);
                varint(&mut record, values.len() as u64);

                for value in values.iter() {
                    write_value(&mut record, &value)?;
                }
            }
        }
        1..=3 => {
            let terms = if kind == 3 {
                PyTuple::new(node.py(), [node.get_item(1)?])?
            } else {
                node.get_item(1)?
                    .cast_into::<PyTuple>()
                    .map_err(PyErr::from)?
            };

            varint(&mut record, if terms.is_empty() { 1 } else { 2 });
            varint(&mut record, 1);
            record.push(INT);
            int(&mut record, i64::from(kind));

            if !terms.is_empty() {
                varint(&mut record, 4);
                record.push(LIST);
                varint(&mut record, terms.len() as u64);

                for term in terms.iter() {
                    expression(&mut record, &term, depth + 1)?;
                }
            }
        }
        _ => {
            return Err(Fault::Query(
                "a filter holds something that is not a condition".into(),
            ));
        }
    }

    embed(out, &record);

    Ok(())
}

/// Writes a value a query compares with, by its Python type, or a parameter.
fn write_value(out: &mut Vec<u8>, value: &Bound<'_, PyAny>) -> Result<(), Fault> {
    if let Ok(param) = value.cast::<Param>() {
        let mut record = Vec::new();

        varint(&mut record, 1);
        varint(&mut record, 1);
        record.push(INT);
        int(&mut record, i64::from(param.get().index));
        embed(out, &record);
    } else if value.is_instance_of::<PyBool>() {
        out.push(if value.extract::<bool>()? {
            TRUE
        } else {
            FALSE
        });
    } else if value.is_instance_of::<PyInt>() {
        let number = value
            .extract::<i64>()
            .map_err(|_| Fault::Query(format!("{value} is beyond a 64-bit int")))?;

        out.push(INT);
        int(out, number);
    } else if value.is_instance_of::<PyFloat>() {
        out.push(FLOAT);
        out.extend_from_slice(&value.extract::<f64>()?.to_le_bytes());
    } else if value.is_instance_of::<PyString>() {
        out.push(STRING);
        string(out, &value.extract::<String>()?);
    } else if is_bytes(value) {
        let bytes = bytes_of(value)?;

        out.push(BYTES);
        varint(out, bytes.len() as u64);
        out.extend_from_slice(&bytes);
    } else {
        return Err(Fault::Query(format!(
            "a query compares with single values, not {value}"
        )));
    }

    Ok(())
}

fn write_path(out: &mut Vec<u8>, path: &[String]) {
    out.push(LIST);
    varint(out, path.len() as u64);

    for name in path {
        out.push(STRING);
        string(out, name);
    }
}

/// Writes `record` as an embedded object: its tag, its length and itself.
fn embed(out: &mut Vec<u8>, record: &[u8]) {
    out.push(OBJECT);
    varint(out, record.len() as u64);
    out.extend_from_slice(record);
}

fn string(out: &mut Vec<u8>, text: &str) {
    varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// A zigzag varint.
fn int(out: &mut Vec<u8>, value: i64) {
    #[expect(
        clippy::cast_sign_loss,
        reason = "zigzag encoding reinterprets the bits"
    )]
    varint(out, ((value << 1) ^ (value >> 63)) as u64);
}

/// An unsigned LEB128 varint.
fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        #[expect(clippy::cast_possible_truncation, reason = "the low seven bits")]
        out.push((value as u8) | 0x80);
        value >>= 7;
    }

    #[expect(clippy::cast_possible_truncation, reason = "less than 0x80")]
    out.push(value as u8);
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Param>()?;
    module.add_class::<NativeQuery>()?;
    module.add_function(wrap_pyfunction!(compile_query, module)?)?;
    module.add_function(wrap_pyfunction!(parse_query, module)?)?;

    Ok(())
}
