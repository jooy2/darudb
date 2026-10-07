//! The schema and the migrations Python declares, as the engine's
//! [`Schema`] and [`Migration`]s.
//!
//! `python/darudb/_schema.py` reads the classes and hands this module plain
//! tuples, so that every rule about what a class may declare is checked in
//! one place, and every rule about what a schema may hold is the engine's,
//! when the file is opened with it.

use darudb::{Collection, Embedded, Migration, Schema, Type, Value};
use pyo3::prelude::*;
use pyo3::types::PyTuple;

use crate::invalid;
use crate::values::plain;

/// A schema, ready to open a file with.
#[pyclass(frozen, module = "darudb._native")]
#[derive(Debug)]
pub(crate) struct NativeSchema {
    pub(crate) schema: Schema,
}

/// One field as Python declares it: the name the file stores, its type, how
/// it holds its value (`"key"`, `"required"`, `"optional"` or `"default"`),
/// its default, and whether it is indexed and unique.
type FieldSpec<'py> = (
    String,
    Bound<'py, PyAny>,
    String,
    Bound<'py, PyAny>,
    bool,
    bool,
);

#[pymethods]
impl NativeSchema {
    /// A schema at `version` with `collections`, each a name and its fields.
    #[new]
    fn new(version: u64, collections: Vec<(String, Vec<FieldSpec<'_>>)>) -> PyResult<Self> {
        let mut schema = Schema::new(version);

        for (name, fields) in collections {
            let mut collection = Collection::new(name);

            for (stored, kind, mode, default, index, unique) in fields {
                let kind = type_of(&kind)?;

                collection = match mode.as_str() {
                    "key" => collection.primary_key(stored.clone(), kind),
                    "required" => collection.field(stored.clone(), kind),
                    "optional" => collection.optional(stored.clone(), kind),
                    _ => {
                        let value = default_of(&kind, &default)?;

                        collection.with_default(stored.clone(), kind, value)
                    }
                };

                if unique {
                    collection = collection.unique(stored);
                } else if index {
                    collection = collection.index(stored);
                }
            }

            schema = schema.collection(collection);
        }

        Ok(Self { schema })
    }
}

/// The engine's type of a field's type as Python declares it.
fn type_of(kind: &Bound<'_, PyAny>) -> PyResult<Type> {
    let py = kind.py();

    if let Ok(name) = kind.extract::<String>() {
        return match name.as_str() {
            "bool" => Ok(Type::Bool),
            "int" => Ok(Type::Int),
            "float" => Ok(Type::Float),
            "string" => Ok(Type::String),
            "bytes" => Ok(Type::Bytes),
            _ => Err(invalid(py, format!("an unknown field type: {name}"))),
        };
    }

    let pair = kind.cast::<PyTuple>()?;
    let tag: String = pair.get_item(0)?.extract()?;
    let inner = pair.get_item(1)?;

    match tag.as_str() {
        "link" => Ok(Type::link(inner.extract::<String>()?)),
        "list" => Ok(Type::list(type_of(&inner)?)),
        "object" => {
            let mut embedded = Embedded::new();

            for (stored, kind, mode, default, _, _) in inner.extract::<Vec<FieldSpec<'_>>>()? {
                let kind = type_of(&kind)?;

                embedded = match mode.as_str() {
                    "required" => embedded.field(stored, kind),
                    "optional" => embedded.optional(stored, kind),
                    _ => {
                        let value = default_of(&kind, &default)?;

                        embedded.with_default(stored, kind, value)
                    }
                };
            }

            Ok(Type::object(embedded))
        }
        _ => Err(invalid(py, format!("an unknown field type: {tag}"))),
    }
}

/// A default as the engine holds it for a field of type `kind`: a float
/// field takes a Python `int` too.
fn default_of(kind: &Type, value: &Bound<'_, PyAny>) -> PyResult<Value> {
    match kind {
        Type::Float if value.is_exact_instance_of::<pyo3::types::PyInt>() => {
            Ok(Value::Float(value.extract::<f64>()?))
        }
        Type::List(element) if matches!(**element, Type::Float) => value
            .try_iter()?
            .map(|item| default_of(element, &item?))
            .collect::<PyResult<_>>()
            .map(Value::List),
        _ => plain(value),
    }
}

/// The structural part of a migration, as Python declares it: its version,
/// the collections it renames, the fields it renames, the collections it
/// deletes and the fields it replaces. The function of a step stays in
/// Python, and runs between the steps that `open_migrating` stops for.
pub(crate) type MigrationSpec = (
    u64,
    Vec<(String, String)>,
    Vec<(String, String, String)>,
    Vec<String>,
    Vec<(String, String)>,
);

/// The engine's migration of `spec`.
pub(crate) fn migration_of(spec: MigrationSpec) -> Migration {
    let (version, renamed_collections, renamed_fields, deleted, replaced) = spec;
    let mut migration = Migration::to(version);

    for (from, to) in renamed_collections {
        migration = migration.rename_collection(from, to);
    }

    for (collection, from, to) in renamed_fields {
        migration = migration.rename_field(collection, from, to);
    }

    for name in deleted {
        migration = migration.delete_collection(name);
    }

    for (collection, field) in replaced {
        migration = migration.replace_field(collection, field);
    }

    migration
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeSchema>()
}
