//! The values objects hold, and objects themselves, as the public API gives
//! and takes them.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A value of a field.
///
/// A link holds the linked object's primary key, so it is an [`Int`],
/// [`String`] or [`Bytes`] here; the schema says it is a link.
///
/// [`Int`]: Value::Int
/// [`String`]: Value::String
/// [`Bytes`]: Value::Bytes
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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
    /// An instant, in microseconds since the Unix epoch, 1970-01-01T00:00:00
    /// UTC, negative before it. It has no time zone: a binding converts the
    /// time of its language to UTC.
    Date(i64),
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

    /// The microseconds since the Unix epoch, if the value is a date.
    pub fn as_date(&self) -> Option<i64> {
        match self {
            Value::Date(value) => Some(*value),
            _ => None,
        }
    }

    /// The date as a [`SystemTime`], if the value is a date that the
    /// platform's `SystemTime` can hold.
    pub fn as_time(&self) -> Option<SystemTime> {
        self.as_date().and_then(time_of)
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

/// A date, rounded down to the microsecond.
///
/// It fails with [`Error::InvalidArgument`](crate::Error::InvalidArgument)
/// for a time more than about 292,000 years from 1970, which a date cannot
/// hold.
impl TryFrom<SystemTime> for Value {
    type Error = crate::Error;

    fn try_from(time: SystemTime) -> crate::Result<Self> {
        micros_of(time)
            .map(Value::Date)
            .ok_or_else(|| crate::Error::InvalidArgument {
                message: format!("{time:?} is outside the range of a date"),
            })
    }
}

/// The microseconds from the Unix epoch to `time`, rounded down, so that a
/// time before the epoch with a part of a microsecond goes to the
/// microsecond before it; `None` outside an `i64`.
pub(crate) fn micros_of(time: SystemTime) -> Option<i64> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_micros()).ok(),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_micros()).ok()?;
            let part = i64::from(before.subsec_nanos() % 1_000 != 0);

            whole.checked_add(part).map(|micros| -micros)
        }
    }
}

/// The time `micros` microseconds from the Unix epoch, if the platform's
/// `SystemTime` holds it.
pub(crate) fn time_of(micros: i64) -> Option<SystemTime> {
    let distance = Duration::from_micros(micros.unsigned_abs());

    if micros < 0 {
        UNIX_EPOCH.checked_sub(distance)
    } else {
        UNIX_EPOCH.checked_add(distance)
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
#[derive(Clone, PartialEq, Default)]
pub struct Object {
    /// The fields, sorted by name in byte order, each name once.
    ///
    /// A sorted vector rather than a map: an object has a few fields, which
    /// a binary search finds as fast, and decoding one costs one allocation
    /// for all of them rather than one for each name and more for the map.
    fields: Vec<(Name, Value)>,
}

impl Object {
    /// An object with no field set.
    pub fn new() -> Self {
        Self::default()
    }

    /// The object with these fields, whose names are all different, in any
    /// order: what reading a record gives.
    pub(crate) fn from_fields(mut fields: Vec<(Name, Value)>) -> Self {
        fields.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        debug_assert!(fields.windows(2).all(|pair| pair[0].0 != pair[1].0));

        Self { fields }
    }

    /// The object with these fields, which are sorted by name already.
    pub(crate) fn from_sorted(fields: Vec<(Name, Value)>) -> Self {
        debug_assert!(
            fields
                .windows(2)
                .all(|pair| pair[0].0.as_bytes() < pair[1].0.as_bytes())
        );

        Self { fields }
    }

    /// This object with field `name` set to `value`.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.set(name, value);
        self
    }

    fn find(&self, name: &str) -> Result<usize, usize> {
        self.fields
            .binary_search_by(|(other, _)| other.as_bytes().cmp(name.as_bytes()))
    }

    /// Sets field `name` to `value`, and returns the value it replaced.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        let name = name.into();
        let value = value.into();

        match self.find(&name) {
            Ok(at) => Some(std::mem::replace(&mut self.fields[at].1, value)),
            Err(at) => {
                self.fields.insert(at, (Name::from(name), value));

                None
            }
        }
    }

    /// Sets every field `changes` has to its value there.
    pub(crate) fn absorb(&mut self, changes: Object) {
        for (name, value) in changes.fields {
            match self.find(name.as_str()) {
                Ok(at) => self.fields[at].1 = value,
                Err(at) => self.fields.insert(at, (name, value)),
            }
        }
    }

    /// The value of field `name`, if the object has the field.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.find(name).ok().map(|at| &self.fields[at].1)
    }

    /// Takes field `name` out of the object.
    pub fn remove(&mut self, name: &str) -> Option<Value> {
        self.find(name).ok().map(|at| self.fields.remove(at).1)
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

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.fields()).finish()
    }
}

/// How long a name is kept inside a [`Name`], without an allocation.
const INLINE: usize = 22;

/// A field's name, kept inline when it is short, as field names nearly always
/// are, so that decoding an object allocates nothing for its names, and
/// copying one is a copy of a few words rather than an allocation or a count
/// that threads decoding objects of one collection would contend for.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Name(Repr);

#[derive(Clone, PartialEq, Eq)]
enum Repr {
    /// The name's bytes, and zeros after them.
    Inline {
        len: u8,
        bytes: [u8; INLINE],
    },
    Heap(Box<str>),
}

impl Name {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match &self.0 {
            Repr::Inline { len, bytes } => &bytes[..usize::from(*len)],
            Repr::Heap(name) => name.as_bytes(),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        match &self.0 {
            // Made from a `str`, so always UTF-8.
            Repr::Inline { .. } => std::str::from_utf8(self.as_bytes()).unwrap_or_default(),
            Repr::Heap(name) => name,
        }
    }
}

impl From<&str> for Name {
    fn from(name: &str) -> Self {
        match u8::try_from(name.len()) {
            Ok(len) if name.len() <= INLINE => {
                let mut bytes = [0; INLINE];

                bytes[..name.len()].copy_from_slice(name.as_bytes());

                Name(Repr::Inline { len, bytes })
            }
            _ => Name(Repr::Heap(name.into())),
        }
    }
}

impl From<String> for Name {
    fn from(name: String) -> Self {
        if name.len() <= INLINE {
            Name::from(name.as_str())
        } else {
            Name(Repr::Heap(name.into_boxed_str()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_keeps_its_fields_by_name_whatever_their_length() {
        let short = "a".repeat(INLINE);
        let long = "b".repeat(INLINE + 1);
        let names = [
            "zeta",
            "",
            "é",
            short.as_str(),
            long.as_str(),
            "Alpha",
            "alpha",
        ];
        let mut object = Object::new();

        for (value, name) in names.iter().enumerate() {
            assert_eq!(object.set(*name, i64::try_from(value).unwrap()), None);
        }

        let mut sorted = names.to_vec();

        sorted.sort_unstable();

        assert_eq!(
            object.fields().map(|(name, _)| name).collect::<Vec<_>>(),
            sorted
        );
        assert_eq!(object.len(), names.len());

        for (value, name) in names.iter().enumerate() {
            assert_eq!(
                object.get(name),
                Some(&Value::Int(i64::try_from(value).unwrap()))
            );
        }

        assert_eq!(object.get("zet"), None);
        assert_eq!(object.set(long.as_str(), "again"), Some(Value::Int(4)));
        assert_eq!(object.remove(short.as_str()), Some(Value::Int(3)));
        assert_eq!(object.remove(short.as_str()), None);
        assert_eq!(
            object.get(long.as_str()),
            Some(&Value::String("again".into()))
        );
    }

    #[test]
    fn objects_with_the_same_fields_are_equal_however_they_were_built() {
        let built = Object::new().with("b", 2).with("a", 1);
        let read = Object::from_fields(vec![
            (Name::from("a"), Value::Int(1)),
            (Name::from("b".to_owned()), Value::Int(2)),
        ]);

        assert_eq!(built, read);
        assert_ne!(built, read.clone().with("c", 3));
        assert_eq!(format!("{built:?}"), r#"{"a": Int(1), "b": Int(2)}"#);
    }

    #[test]
    fn times_are_microseconds_rounded_down() {
        let after = |secs, nanos| UNIX_EPOCH + Duration::new(secs, nanos);
        let before = |secs, nanos| UNIX_EPOCH - Duration::new(secs, nanos);

        assert_eq!(micros_of(UNIX_EPOCH), Some(0));
        assert_eq!(micros_of(after(0, 999)), Some(0));
        assert_eq!(micros_of(after(0, 1_000)), Some(1));
        assert_eq!(micros_of(after(1, 1)), Some(1_000_000));
        assert_eq!(micros_of(before(0, 1)), Some(-1));
        assert_eq!(micros_of(before(0, 1_000)), Some(-1));
        assert_eq!(micros_of(before(0, 1_001)), Some(-2));
        assert_eq!(micros_of(before(1, 0)), Some(-1_000_000));

        for micros in [i64::MIN + 1, -1_000_001, -1, 0, 1, 1_793_361_600_000_000] {
            let Some(time) = time_of(micros) else {
                // A platform whose `SystemTime` does not reach that far.
                continue;
            };

            assert_eq!(micros_of(time), Some(micros));
        }

        assert_eq!(micros_of(after(u64::MAX / 2, 0)), None);
    }
}
