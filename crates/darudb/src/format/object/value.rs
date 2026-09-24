//! The values objects hold, and objects themselves, as the public API gives
//! and takes them.

use std::collections::BTreeMap;

/// A value of a field.
///
/// A link holds the linked object's primary key, so it is an [`Int`],
/// [`String`] or [`Bytes`] here; the schema says it is a link. A date or a
/// time is an [`Int`], in the unit the application chooses.
///
/// [`Int`]: Value::Int
/// [`String`]: Value::String
/// [`Bytes`]: Value::Bytes
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// No value: an optional field left out.
    Null,
    /// `false` or `true`.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// A 64-bit floating-point number.
    Float(f64),
    /// UTF-8 text.
    String(String),
    /// Any bytes.
    Bytes(Vec<u8>),
    /// A list of values of one type.
    List(Vec<Value>),
    /// An embedded object.
    Object(Object),
}

impl Value {
    /// Whether the value is [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// The boolean, if the value is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The integer, if the value is one.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// The floating-point number, if the value is one.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// The text, if the value is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(value) => Some(value),
            _ => None,
        }
    }

    /// The bytes, if the value holds bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(value) => Some(value),
            _ => None,
        }
    }

    /// The elements, if the value is a list.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(value) => Some(value),
            _ => None,
        }
    }

    /// The embedded object, if the value is one.
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(value) => Some(value),
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Int(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Int(i64::from(value))
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Int(i64::from(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Float(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Value::Bytes(value.to_vec())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Value::Bytes(value)
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Value::List(value)
    }
}

impl From<Object> for Value {
    fn from(value: Object) -> Self {
        Value::Object(value)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map_or(Value::Null, Into::into)
    }
}

/// An object: values by field name.
///
/// An object read from a database is a copy. It holds every field of its
/// collection's schema, [`Value::Null`] for an optional field without a
/// value, and it stays valid after the transaction and the database are
/// gone.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    fields: BTreeMap<String, Value>,
}

impl Object {
    /// An object with no field set.
    pub fn new() -> Self {
        Self::default()
    }

    /// This object with field `name` set to `value`.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.set(name, value);
        self
    }

    /// Sets field `name` to `value`, and returns the value it replaced.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        self.fields.insert(name.into(), value.into())
    }

    /// The value of field `name`, if the object has the field.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    /// Takes field `name` out of the object.
    pub fn remove(&mut self, name: &str) -> Option<Value> {
        self.fields.remove(name)
    }

    /// The fields, by name in byte order.
    pub fn fields(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.fields
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// The number of fields the object has.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the object has no field.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}
