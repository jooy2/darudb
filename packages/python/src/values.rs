//! Python values to the engine's [`Value`] and [`Object`], and back.
//!
//! A class the Python code declares as a collection or an embedded object
//! comes with a [`Layout`]: its class, and for each field the Python
//! attribute, the name the file stores, and what kind of value it holds. The
//! kind matters only where Python's value does not say it alone: a `float`
//! field takes a Python `int` too, and an embedded object is an instance of
//! its own class. Every other value is converted by its Python type, and the
//! engine checks it against the schema, so a value of the wrong type is the
//! engine's `INVALID_ARGUMENT`.
//!
//! An object read from the file is built without its class's `__init__`:
//! the instance is made with `object.__new__` and its fields are put in its
//! `__dict__`, which a frozen dataclass allows. Calling `__init__` would cost
//! a call and an attribute assignment per field for every object read.

use std::collections::HashMap;

use darudb::{Object, Value};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PyList, PyMemoryView, PyString, PyTuple,
    PyType,
};

use crate::invalid;

/// What a field holds, as far as converting it needs to know.
#[derive(Debug)]
pub(crate) enum Kind {
    Bool,
    Int,
    Float,
    String,
    Bytes,
    /// A link: the primary key of an object, an int, a string or bytes.
    Key,
    List(Box<Kind>),
    Object(Py<Layout>),
}

/// One field of a [`Layout`].
#[derive(Debug)]
struct Field {
    /// The Python attribute.
    attr: Py<PyString>,
    /// The name the file stores the field under.
    stored: String,
    kind: Kind,
}

/// A Python class as a collection's objects or an embedded object: its
/// fields, in the order the class declares them.
#[pyclass(frozen, module = "darudb._native")]
#[derive(Debug)]
pub(crate) struct Layout {
    class: Py<PyType>,
    fields: Vec<Field>,
    /// Each field by its Python attribute, for the changes of an update.
    by_attr: HashMap<String, usize>,
}

#[pymethods]
impl Layout {
    /// A layout of `class` with `fields`, each a Python attribute, the name
    /// the file stores, and a kind: `"bool"`, `"int"`, `"float"`, `"str"`,
    /// `"bytes"`, `"key"`, `("list", kind)` or the `Layout` of an embedded
    /// object.
    #[new]
    fn new(
        class: Bound<'_, PyType>,
        fields: Vec<(String, String, Bound<'_, PyAny>)>,
    ) -> PyResult<Self> {
        let py = class.py();
        let mut by_attr = HashMap::new();
        let mut built = Vec::with_capacity(fields.len());

        for (index, (attr, stored, kind)) in fields.into_iter().enumerate() {
            by_attr.insert(attr.clone(), index);
            built.push(Field {
                attr: PyString::intern(py, &attr).unbind(),
                stored,
                kind: kind_of(&kind)?,
            });
        }

        Ok(Self {
            class: class.unbind(),
            fields: built,
            by_attr,
        })
    }
}

fn kind_of(kind: &Bound<'_, PyAny>) -> PyResult<Kind> {
    let py = kind.py();

    if let Ok(layout) = kind.cast::<Layout>() {
        return Ok(Kind::Object(layout.clone().unbind()));
    }

    if let Ok(pair) = kind.cast::<PyTuple>() {
        if pair.len() == 2 && pair.get_item(0)?.extract::<String>()? == "list" {
            return Ok(Kind::List(Box::new(kind_of(&pair.get_item(1)?)?)));
        }
    } else if let Ok(name) = kind.extract::<String>() {
        return match name.as_str() {
            "bool" => Ok(Kind::Bool),
            "int" => Ok(Kind::Int),
            "float" => Ok(Kind::Float),
            "str" => Ok(Kind::String),
            "bytes" => Ok(Kind::Bytes),
            "key" => Ok(Kind::Key),
            _ => Err(invalid(py, format!("an unknown kind of field: {name}"))),
        };
    }

    Err(invalid(
        py,
        "a field's kind is a name, a list of a kind or a layout",
    ))
}

/// `object.__new__`, which makes an instance without running `__init__`.
static OBJECT_NEW: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

impl Layout {
    /// The class's name, for messages.
    fn class_name(&self, py: Python<'_>) -> String {
        self.class
            .bind(py)
            .name()
            .map_or_else(|_| "object".to_owned(), |name| name.to_string())
    }

    /// The object `instance`, an instance of the class, as the engine holds
    /// it. A field holding `None` is left out, which is how the engine reads
    /// null: an optional field without a value, a field with a default, or
    /// an auto-increment key the engine assigns.
    pub(crate) fn to_object(&self, instance: &Bound<'_, PyAny>) -> PyResult<Object> {
        let py = instance.py();

        if !instance.is_instance(self.class.bind(py))? {
            return Err(invalid(
                py,
                format!(
                    "expected a {}, not {}",
                    self.class_name(py),
                    type_name(instance)
                ),
            ));
        }

        let mut object = Object::new();

        for field in &self.fields {
            let value = to_value(&field.kind, &instance.getattr(field.attr.bind(py))?)?;

            if !value.is_null() {
                object.set(field.stored.clone(), value);
            }
        }

        Ok(object)
    }

    /// The changes of an update, by Python attribute, as the engine takes
    /// them: `None` sets a field to null, which gives a field with a default
    /// its default.
    pub(crate) fn to_changes(&self, changes: &Bound<'_, PyDict>) -> PyResult<Object> {
        let py = changes.py();
        let mut object = Object::new();

        for (attr, value) in changes.iter() {
            let attr: String = attr.extract()?;
            let Some(&index) = self.by_attr.get(&attr) else {
                return Err(invalid(
                    py,
                    format!("{} has no field {attr}", self.class_name(py)),
                ));
            };
            let field = &self.fields[index];

            object.set(field.stored.clone(), to_value(&field.kind, &value)?);
        }

        Ok(object)
    }

    /// An instance of the class holding `object`, as read from the file.
    pub(crate) fn to_instance<'py>(
        &self,
        py: Python<'py>,
        object: &Object,
    ) -> PyResult<Bound<'py, PyAny>> {
        let new = OBJECT_NEW.get_or_try_init(py, || {
            py.import("builtins")?
                .getattr("object")?
                .getattr("__new__")
                .map(Bound::unbind)
        })?;
        let instance = new.bind(py).call1((self.class.bind(py),))?;
        let fields = instance.getattr("__dict__")?.cast_into::<PyDict>()?;

        for field in &self.fields {
            let value = match object.get(&field.stored) {
                Some(value) => from_value(py, &field.kind, value)?,
                None => py.None().into_bound(py),
            };

            fields.set_item(field.attr.bind(py), value)?;
        }

        Ok(instance)
    }
}

/// The Python type of `value`, for messages.
fn type_name(value: &Bound<'_, PyAny>) -> String {
    value
        .get_type()
        .name()
        .map_or_else(|_| "a value".to_owned(), |name| format!("a {name}"))
}

/// A value of a field of kind `kind`.
fn to_value(kind: &Kind, value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = value.py();

    if value.is_none() {
        return Ok(Value::Null);
    }

    match kind {
        // A float field takes an int, which Python code writes for a whole
        // number as often as not.
        Kind::Float if value.is_exact_instance_of::<PyInt>() => {
            Ok(Value::Float(value.extract::<f64>()?))
        }
        Kind::Object(layout) => Ok(Value::Object(layout.get().to_object(value)?)),
        Kind::List(element) => {
            if value.is_instance_of::<PyString>() || is_bytes(value) {
                return Err(invalid(
                    py,
                    format!("a list field holds a list, not {}", type_name(value)),
                ));
            }

            let mut items = Vec::new();

            for item in value.try_iter().map_err(|_| {
                invalid(
                    py,
                    format!("a list field holds a list, not {}", type_name(value)),
                )
            })? {
                items.push(to_value(element, &item?)?);
            }

            Ok(Value::List(items))
        }
        _ => plain(value),
    }
}

/// Whether `value` is bytes of one of the types Python keeps them in.
pub(crate) fn is_bytes(value: &Bound<'_, PyAny>) -> bool {
    value.is_instance_of::<PyBytes>()
        || value.is_instance_of::<PyByteArray>()
        || value.is_instance_of::<PyMemoryView>()
}

/// A Python value as the engine's, by its type alone: what a key, a query's
/// value and a value of a scalar field are.
pub(crate) fn plain(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = value.py();

    if value.is_none() {
        Ok(Value::Null)
    } else if value.is_instance_of::<PyBool>() {
        Ok(Value::Bool(value.extract()?))
    } else if value.is_instance_of::<PyInt>() {
        value
            .extract::<i64>()
            .map(Value::Int)
            .map_err(|_| invalid(py, format!("{value} is beyond a 64-bit int")))
    } else if value.is_instance_of::<PyFloat>() {
        Ok(Value::Float(value.extract()?))
    } else if value.is_instance_of::<PyString>() {
        Ok(Value::String(value.extract()?))
    } else if is_bytes(value) {
        Ok(Value::Bytes(bytes_of(value)?))
    } else if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        value
            .try_iter()?
            .map(|item| plain(&item?))
            .collect::<PyResult<_>>()
            .map(Value::List)
    } else {
        Err(invalid(
            py,
            format!("DaruDB cannot store {}", type_name(value)),
        ))
    }
}

/// The bytes of a `bytes`, a `bytearray` or a `memoryview`.
pub(crate) fn bytes_of(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(bytes) = value.cast::<PyBytes>() {
        Ok(bytes.as_bytes().to_vec())
    } else if let Ok(array) = value.cast::<PyByteArray>() {
        Ok(array.to_vec())
    } else {
        PyBytes::new(value.py(), &[])
            .get_type()
            .call1((value,))?
            .cast_into::<PyBytes>()
            .map(|bytes| bytes.as_bytes().to_vec())
            .map_err(PyErr::from)
    }
}

/// A primary key: an int, a string or bytes.
pub(crate) fn key(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = value.py();

    match plain(value) {
        Ok(key @ (Value::Int(_) | Value::String(_) | Value::Bytes(_))) => Ok(key),
        Ok(_) | Err(_) => Err(invalid(
            py,
            format!(
                "a primary key is an int, a str or bytes, not {}",
                type_name(value)
            ),
        )),
    }
}

/// A value read from the file, as Python holds a field of kind `kind`.
fn from_value<'py>(py: Python<'py>, kind: &Kind, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    match (kind, value) {
        (Kind::Object(layout), Value::Object(object)) => layout.get().to_instance(py, object),
        (Kind::List(element), Value::List(items)) => {
            let list = PyList::empty(py);

            for item in items {
                list.append(from_value(py, element, item)?)?;
            }

            Ok(list.into_any())
        }
        _ => to_python(py, value),
    }
}

/// A value as Python holds it without a layout: an embedded object is a
/// `dict` by the names the file stores. What the migration's view of the
/// previous schema reads.
pub(crate) fn to_python<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Null => py.None().into_bound(py),
        Value::Bool(value) => PyBool::new(py, *value).to_owned().into_any(),
        Value::Int(value) => value.into_pyobject(py)?.into_any(),
        Value::Float(value) => PyFloat::new(py, *value).into_any(),
        Value::String(value) => PyString::new(py, value).into_any(),
        Value::Bytes(value) => PyBytes::new(py, value).into_any(),
        Value::List(items) => {
            let list = PyList::empty(py);

            for item in items {
                list.append(to_python(py, item)?)?;
            }

            list.into_any()
        }
        Value::Object(object) => dict_of(py, object)?.into_any(),
    })
}

/// An object as a `dict` by the names the file stores.
pub(crate) fn dict_of<'py>(py: Python<'py>, object: &Object) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);

    for (name, value) in object.fields() {
        dict.set_item(name, to_python(py, value)?)?;
    }

    Ok(dict)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Layout>()
}
