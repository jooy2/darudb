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
//!
//! An object of a class whose fields all hold scalars is written the other
//! way: its values are read as [`Held`] scalars with the GIL, and the engine
//! writes them into the record through the class's shape without it.
//!
//! A date is an aware `datetime`, converted with whole `timedelta`
//! arithmetic from the Unix epoch in UTC rather than through `timestamp()`,
//! whose float loses microseconds far from 1970. The stable ABI has no
//! `datetime` C API, so the `datetime` module's own objects do the work.

use std::borrow::Cow;
use std::collections::HashMap;

use std::sync::atomic::{AtomicU64, Ordering};

use darudb::__binding::{Scalar, Shape, ShapeField};
use darudb::{FieldReader, Object, Value, ValueReader, ValueWriter};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PyList, PyMemoryView, PyString, PyTuple,
    PyType,
};

use crate::{OrRaise, invalid};

/// What a field holds, as far as converting it needs to know.
#[derive(Debug)]
pub(crate) enum Kind {
    Bool,
    Int,
    Float,
    String,
    Bytes,
    Date,
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
    /// What the engine keeps the layout's shape under, in each handle's
    /// schema: a number no other layout of the process has.
    key: u64,
    /// The fields as the engine matches them with a stored collection's, by
    /// the names the file stores, in the layout's order.
    shape: Vec<ShapeField>,
    /// Whether every field holds a scalar, so that an object's record is
    /// written through the shape: the engine reads a record with a list, an
    /// embedded object or a link into an `Object` anyway.
    flat: bool,
}

/// The number the next layout's shape is kept under.
static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

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

        let shape = built
            .iter()
            .map(|field| ShapeField {
                name: field.stored.clone(),
                // A list of embedded objects is read with its elements'.
                fields: match &field.kind {
                    Kind::Object(layout) => Some(layout.get().shape.clone()),
                    Kind::List(element) => match element.as_ref() {
                        Kind::Object(layout) => Some(layout.get().shape.clone()),
                        _ => None,
                    },
                    _ => None,
                },
            })
            .collect();
        let flat = !built.is_empty()
            && built.iter().all(|field| {
                matches!(
                    field.kind,
                    Kind::Bool | Kind::Int | Kind::Float | Kind::String | Kind::Bytes | Kind::Date
                )
            });

        Ok(Self {
            class: class.unbind(),
            fields: built,
            by_attr,
            key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
            shape,
            flat,
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
            "date" => Ok(Kind::Date),
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

        self.check_instance(instance)?;

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
}

impl Layout {
    /// Refuses `instance` unless it is an instance of the class.
    fn check_instance(&self, instance: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = instance.py();

        if instance.is_instance(self.class.bind(py))? {
            return Ok(());
        }

        Err(invalid(
            py,
            format!(
                "expected a {}, not {}",
                self.class_name(py),
                type_name(instance)
            ),
        ))
    }

    /// How many fields the layout has.
    pub(crate) fn len(&self) -> usize {
        self.fields.len()
    }

    /// The values of the fields of `instance`, an instance of the class, put
    /// after `values` in the layout's order, for [`held`](Self::held) to
    /// read, when every field holds a scalar. Returns whether they were put
    /// there; an object of another layout is converted with
    /// [`to_object`](Self::to_object).
    pub(crate) fn attributes<'py>(
        &self,
        instance: &Bound<'py, PyAny>,
        values: &mut Vec<Bound<'py, PyAny>>,
    ) -> PyResult<bool> {
        if !self.flat {
            return Ok(false);
        }

        let py = instance.py();

        self.check_instance(instance)?;

        for field in &self.fields {
            values.push(instance.getattr(field.attr.bind(py))?);
        }

        Ok(true)
    }

    /// The scalars `values` hold, the values of the fields of objects one
    /// after another as [`attributes`](Self::attributes) put them, borrowed
    /// where Python keeps the bytes, so that the engine writes them without
    /// the GIL. Returns whether each value is of its field's kind, or null;
    /// a value that is not, which the engine refuses, is left to
    /// [`to_object`](Self::to_object), so that the engine's message names
    /// the field, which a refused record's does not.
    pub(crate) fn held<'a>(
        &self,
        values: &'a [Bound<'_, PyAny>],
        held: &mut Vec<Held<'a>>,
    ) -> PyResult<bool> {
        for (value, field) in values.iter().zip(self.fields.iter().cycle()) {
            let value = held_value(&field.kind, value)?;
            let fits = matches!(
                (&field.kind, &value),
                (_, Some(Held::Null))
                    | (Kind::Bool, Some(Held::Bool(_)))
                    | (Kind::Int, Some(Held::Int(_)))
                    | (Kind::Float, Some(Held::Float(_)))
                    | (Kind::String, Some(Held::Str(_)))
                    | (Kind::Bytes, Some(Held::Bytes(_)))
                    | (Kind::Date, Some(Held::Date(_)))
            );

            match value {
                Some(value) if fits => held.push(value),
                _ => return Ok(false),
            }
        }

        Ok(true)
    }

    /// What the engine keeps the layout's shape under.
    pub(crate) fn shape_key(&self) -> u64 {
        self.key
    }

    /// The fields the engine works the layout's shape out from.
    pub(crate) fn shape_fields(&self) -> Vec<ShapeField> {
        self.shape.clone()
    }

    /// An instance of the class holding the object whose record is
    /// `record`, read through `shape` straight into Python values, with no
    /// engine `Object` made of it first. `values` is room for the fields'
    /// values, which a caller reading many objects lends each.
    pub(crate) fn read_record<'py>(
        &self,
        py: Python<'py>,
        shape: &Shape,
        record: &[u8],
        values: &mut Vec<Option<Bound<'py, PyAny>>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut failed = None;
        let read = shape.read(record, |fields| {
            if let Err(error) = self.read_fields(py, fields, values) {
                failed = Some(error);
            }

            Ok(())
        });

        if let Some(error) = failed {
            return Err(error);
        }

        read.or_raise(py)?;
        self.instance_of(py, values)
    }

    /// Reads each field `fields` gives into `values`, at its slot, which is
    /// the field's place in the layout.
    fn read_fields<'py>(
        &self,
        py: Python<'py>,
        mut fields: FieldReader<'_>,
        values: &mut Vec<Option<Bound<'py, PyAny>>>,
    ) -> PyResult<()> {
        values.clear();
        values.resize(self.fields.len(), None);

        while let Some((slot, value)) = fields.next().or_raise(py)? {
            let field = self
                .fields
                .get(slot)
                .ok_or_else(|| invalid(py, "a record holds a field the class does not"))?;

            values[slot] = Some(read_value(py, &field.kind, value)?);
        }

        Ok(())
    }

    /// An instance of the class whose fields hold `values`, set in the
    /// layout's order, as `__init__` would set them, so that instances share
    /// the keys of their `__dict__`.
    fn instance_of<'py>(
        &self,
        py: Python<'py>,
        values: &mut [Option<Bound<'py, PyAny>>],
    ) -> PyResult<Bound<'py, PyAny>> {
        let new = OBJECT_NEW.get_or_try_init(py, || {
            py.import("builtins")?
                .getattr("object")?
                .getattr("__new__")
                .map(Bound::unbind)
        })?;
        let instance = new.bind(py).call1((self.class.bind(py),))?;
        let dict = instance
            .getattr(intern!(py, "__dict__"))?
            .cast_into::<PyDict>()?;

        for (field, value) in self.fields.iter().zip(values.iter_mut()) {
            match value.take() {
                Some(value) => dict.set_item(field.attr.bind(py), value)?,
                None => dict.set_item(field.attr.bind(py), py.None())?,
            }
        }

        Ok(instance)
    }
}

/// A value of a field of kind `kind` as Python holds it, read where the
/// record holds it.
fn read_value<'py>(
    py: Python<'py>,
    kind: &Kind,
    value: ValueReader<'_>,
) -> PyResult<Bound<'py, PyAny>> {
    if value.is_null() {
        return Ok(py.None().into_bound(py));
    }

    Ok(match kind {
        Kind::Bool => PyBool::new(py, value.bool().or_raise(py)?)
            .to_owned()
            .into_any(),
        Kind::Int => value.int().or_raise(py)?.into_pyobject(py)?.into_any(),
        Kind::Float => PyFloat::new(py, value.float().or_raise(py)?).into_any(),
        Kind::String => PyString::new(py, value.str().or_raise(py)?).into_any(),
        Kind::Bytes => PyBytes::new(py, value.bytes().or_raise(py)?).into_any(),
        Kind::Date => datetime_of(py, value.date().or_raise(py)?)?,
        Kind::Key => match value.linked().or_raise(py)?.scalar().or_raise(py)? {
            Scalar::Int(key) => key.into_pyobject(py)?.into_any(),
            Scalar::Str(key) => PyString::new(py, key).into_any(),
            Scalar::Bytes(key) => PyBytes::new(py, key).into_any(),
            _ => return Err(invalid(py, "a link holds a key of another type")),
        },
        Kind::List(element) => {
            let list = PyList::empty(py);

            for item in value.elements().or_raise(py)? {
                list.append(read_value(py, element, item)?)?;
            }

            list.into_any()
        }
        Kind::Object(layout) => {
            let layout = layout.get();
            let mut values = Vec::new();

            layout.read_fields(py, value.fields().or_raise(py)?, &mut values)?;
            layout.instance_of(py, &mut values)?
        }
    })
}

/// A scalar of an object, read from its Python value with the GIL and
/// written into the record without it. A string or `bytes` is borrowed from
/// the Python object, which the caller keeps; a `bytearray` or a
/// `memoryview`, which Python code may change meanwhile, is copied.
#[derive(Debug)]
pub(crate) enum Held<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(&'a str),
    Bytes(Cow<'a, [u8]>),
    /// Microseconds since the Unix epoch.
    Date(i64),
}

impl Held<'_> {
    /// Writes the value as a field of a record.
    pub(crate) fn write(&self, writer: ValueWriter<'_>) -> darudb::Result<()> {
        match self {
            Held::Null => writer.null(),
            Held::Bool(value) => writer.bool(*value),
            Held::Int(value) => writer.int(*value),
            Held::Float(value) => writer.float(*value),
            Held::Str(value) => writer.string(value),
            Held::Bytes(value) => writer.bytes(value),
            Held::Date(value) => writer.date(*value),
        }
    }
}

/// A value of a field of the scalar kind `kind`, as [`to_value`] converts
/// it, or `None` for a value that is no scalar.
fn held_value<'a>(kind: &Kind, value: &'a Bound<'_, PyAny>) -> PyResult<Option<Held<'a>>> {
    let py = value.py();

    Ok(Some(if value.is_none() {
        Held::Null
    } else if value.is_instance_of::<PyBool>() {
        Held::Bool(value.extract()?)
    } else if value.is_instance_of::<PyInt>() {
        // A float field takes an int, as `to_value` says.
        if matches!(kind, Kind::Float) && value.is_exact_instance_of::<PyInt>() {
            Held::Float(value.extract()?)
        } else {
            Held::Int(
                value
                    .extract()
                    .map_err(|_| invalid(py, format!("{value} is beyond a 64-bit int")))?,
            )
        }
    } else if value.is_instance_of::<PyFloat>() {
        Held::Float(value.extract()?)
    } else if let Ok(string) = value.cast::<PyString>() {
        Held::Str(string.to_str()?)
    } else if let Ok(bytes) = value.cast::<PyBytes>() {
        Held::Bytes(Cow::Borrowed(bytes.as_bytes()))
    } else if is_bytes(value) {
        Held::Bytes(Cow::Owned(bytes_of(value)?))
    } else if is_datetime(value)? {
        Held::Date(micros_of(value)?)
    } else if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        return Ok(None);
    } else {
        return Err(invalid(
            py,
            format!("DaruDB cannot store {}", type_name(value)),
        ));
    }))
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

/// An object from a `dict` by the names the file stores, as a collection
/// reached by name takes one: each value is converted for its field's type in
/// `fields`, the stored schema's, so that a float field takes an int and an
/// embedded object a `dict`, as they do through a class. A name the schema
/// lacks passes as it is, for the engine to refuse with its own message.
pub(crate) fn object_of(value: &Bound<'_, PyAny>, fields: &[darudb::Field]) -> PyResult<Object> {
    let py = value.py();
    let dict = value.cast::<PyDict>().map_err(|_| {
        invalid(
            py,
            format!(
                "an object of a collection reached by name is a dict, not {}",
                type_name(value)
            ),
        )
    })?;
    let mut object = Object::new();

    for (name, item) in dict.iter() {
        let name = name.cast::<PyString>().map_err(|_| {
            invalid(
                py,
                format!("a field's name is a str, not {}", type_name(&name)),
            )
        })?;
        let name = name.to_str()?;
        let kind = fields
            .iter()
            .find(|field| field.name() == name)
            .map(darudb::Field::kind);

        object.set(name, value_for(kind, &item)?);
    }

    Ok(object)
}

/// `value` as the engine's, for a field of `kind` when the schema has one.
fn value_for(kind: Option<&darudb::Type>, value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = value.py();

    if value.is_none() {
        return Ok(Value::Null);
    }

    match kind {
        Some(darudb::Type::Float) if value.is_exact_instance_of::<PyInt>() => {
            Ok(Value::Float(value.extract::<f64>()?))
        }
        Some(darudb::Type::Object(embedded)) => {
            Ok(Value::Object(object_of(value, embedded.fields())?))
        }
        Some(darudb::Type::List(element)) => {
            if value.is_instance_of::<PyString>() || is_bytes(value) {
                return Err(invalid(
                    py,
                    format!("a list field holds a list, not {}", type_name(value)),
                ));
            }

            value
                .try_iter()
                .map_err(|_| {
                    invalid(
                        py,
                        format!("a list field holds a list, not {}", type_name(value)),
                    )
                })?
                .map(|item| value_for(Some(element), &item?))
                .collect::<PyResult<_>>()
                .map(Value::List)
        }
        _ if value.is_instance_of::<PyDict>() => Ok(Value::Object(object_of(value, &[])?)),
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
    } else if is_datetime(value)? {
        Ok(Value::Date(micros_of(value)?))
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

/// What converting a date takes from the `datetime` module, imported once:
/// its `datetime` class, its `timedelta`, the Unix epoch in UTC, and a
/// microsecond.
struct Time {
    datetime: Py<PyAny>,
    timedelta: Py<PyAny>,
    epoch: Py<PyAny>,
    microsecond: Py<PyAny>,
}

static TIME: PyOnceLock<Time> = PyOnceLock::new();

fn time(py: Python<'_>) -> PyResult<&Time> {
    TIME.get_or_try_init(py, || {
        let module = py.import("datetime")?;
        let datetime = module.getattr("datetime")?;
        let timedelta = module.getattr("timedelta")?;
        let utc = module.getattr("timezone")?.getattr("utc")?;
        let epoch = datetime.call1((1970, 1, 1, 0, 0, 0, 0, utc))?;
        let microsecond = timedelta.call1((0, 0, 1))?;

        Ok(Time {
            datetime: datetime.unbind(),
            timedelta: timedelta.unbind(),
            epoch: epoch.unbind(),
            microsecond: microsecond.unbind(),
        })
    })
}

/// Whether `value` is a `datetime`, which a date field holds. A `date`
/// alone is not: it names a day, not an instant.
pub(crate) fn is_datetime(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    value.is_instance(time(value.py())?.datetime.bind(value.py()))
}

/// The microseconds from the Unix epoch to `value`, an aware `datetime`. A
/// naive one names no instant, and is refused.
pub(crate) fn micros_of(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    let py = value.py();
    let time = time(py)?;

    if value.call_method0(intern!(py, "utcoffset"))?.is_none() {
        return Err(invalid(
            py,
            format!(
                "{value} is a naive datetime, which names no instant; give it a time zone, such as datetime.timezone.utc"
            ),
        ));
    }

    // A `timedelta` divided by one microsecond is a whole number of them,
    // rounded down, which a datetime's range keeps inside an `i64`.
    value
        .sub(time.epoch.bind(py))?
        .floor_div(time.microsecond.bind(py))?
        .extract()
}

/// The aware `datetime` in UTC `micros` microseconds from the Unix epoch. A
/// date beyond the years a `datetime` holds, 1 to 9999, is refused.
pub(crate) fn datetime_of(py: Python<'_>, micros: i64) -> PyResult<Bound<'_, PyAny>> {
    let time = time(py)?;
    let delta = time.timedelta.bind(py).call1((0, 0, micros))?;

    time.epoch.bind(py).add(delta).map_err(|_| {
        invalid(
            py,
            format!("a date {micros} microseconds from 1970 is beyond the years a datetime holds"),
        )
    })
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
        Value::Date(micros) => datetime_of(py, *micros)?,
        _ => {
            return Err(crate::failure(
                py,
                "INTERNAL",
                format!("a value this package does not know: {value:?}"),
            ));
        }
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
