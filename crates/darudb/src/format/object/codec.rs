//! Records: the encoding of objects in the file and across the language
//! boundary (`design/objects.md`, "Records").
//!
//! Every value carries a tag, so a record reads without its schema. This
//! module reads and writes records as [`Raw`] trees, and converts between a
//! tree and an [`Object`] with the fields of a schema: the conversion checks
//! types, fills defaults, and skips the fields a schema no longer has.

use super::schema::{FieldDef, Fields, Kind};
use super::value::{Name, Object, Value};

const FALSE: u8 = 0x02;
const TRUE: u8 = 0x03;
const INT: u8 = 0x04;
const FLOAT: u8 = 0x05;
const STRING: u8 = 0x06;
const BYTES: u8 = 0x07;
const LIST: u8 = 0x08;
const OBJECT: u8 = 0x09;
const LINK: u8 = 0x0A;

/// How deeply lists and objects may nest in a record read from the file. A
/// schema nests far less; the bound keeps a damaged record from exhausting
/// the stack.
const MAX_DEPTH: usize = 64;

/// How many fields or elements reading a record reserves room for before it
/// has read them. A count is bounded by the bytes left in the record, but a
/// value in memory is many times the size of its smallest encoding, so a
/// damaged count would otherwise reserve far more than the record holds.
const RESERVE: usize = 256;

/// A value as a record holds it, before any schema says what it means.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Raw {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Raw>),
    Object(Vec<(u64, Raw)>),
    Link(Box<Raw>),
}

/// The bytes of the record whose fields are `fields`, by id in ascending
/// order.
pub(crate) fn write(fields: &[(u64, Raw)]) -> Vec<u8> {
    let mut out = Vec::new();

    write_fields(fields, &mut out);

    out
}

fn write_fields(fields: &[(u64, Raw)], out: &mut Vec<u8>) {
    write_varint(fields.len() as u64, out);

    for (id, value) in fields {
        write_varint(*id, out);
        write_value(value, out);
    }
}

fn write_value(value: &Raw, out: &mut Vec<u8>) {
    match value {
        Raw::Bool(false) => out.push(FALSE),
        Raw::Bool(true) => out.push(TRUE),
        Raw::Int(value) => {
            out.push(INT);
            write_varint(zigzag(*value), out);
        }
        Raw::Float(value) => {
            out.push(FLOAT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Raw::String(value) => {
            out.push(STRING);
            write_varint(value.len() as u64, out);
            out.extend_from_slice(value.as_bytes());
        }
        Raw::Bytes(value) => {
            out.push(BYTES);
            write_varint(value.len() as u64, out);
            out.extend_from_slice(value);
        }
        Raw::List(values) => {
            out.push(LIST);
            write_varint(values.len() as u64, out);

            for value in values {
                write_value(value, out);
            }
        }
        Raw::Object(fields) => {
            let mut inner = Vec::new();

            write_fields(fields, &mut inner);
            out.push(OBJECT);
            write_varint(inner.len() as u64, out);
            out.extend_from_slice(&inner);
        }
        Raw::Link(key) => {
            out.push(LINK);
            write_value(key, out);
        }
    }
}

/// The fields of the record `bytes`, which has to be exactly one record.
pub(crate) fn read(bytes: &[u8]) -> Result<Vec<(u64, Raw)>, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let fields = reader.fields(0)?;

    if reader.at != bytes.len() {
        return Err("a record has bytes after its last field");
    }

    Ok(fields)
}

/// The object whose record is `bytes`, under `fields`, read straight into
/// values: what [`read`] and then [`to_object`] give, without the record's
/// fields in between, which every object read would allocate and move.
pub(crate) fn object_of(bytes: &[u8], fields: &Fields) -> Result<Object, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let object = reader.object(fields, 0)?;

    if reader.at != bytes.len() {
        return Err("a record has bytes after its last field");
    }

    Ok(object)
}

/// The value of a field the record leaves out: its default, or null.
fn absent(field: &FieldDef) -> Result<Value, &'static str> {
    match &field.default {
        Some(default) => Ok(default.clone()),
        None if field.optional => Ok(Value::Null),
        None => Err("a record lacks a required field"),
    }
}

/// A field's value as a record holds it, borrowed from the record: a scalar,
/// or the encoding of a list, an embedded object or a link, tag included,
/// for [`value_of`] to read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FieldRef<'a> {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(&'a str),
    Bytes(&'a [u8]),
    Encoded(&'a [u8]),
}

/// The value of field `id` in the record `bytes`, if the record holds it,
/// found by stepping over the fields before it without reading them into
/// values. For a filter or a sort that reads a field or two of many records;
/// an object that is kept is read whole, and checked whole, by [`read`].
pub(crate) fn find_field(bytes: &[u8], id: u64) -> Result<Option<FieldRef<'_>>, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let mut last = None;

    for _ in 0..reader.count(2)? {
        let field = reader.varint()?;

        if last.is_some_and(|last| last >= field) {
            return Err("a record's field ids are out of order");
        }

        last = Some(field);

        if field > id {
            break;
        }

        if field < id {
            reader.skip(0)?;

            continue;
        }

        let start = reader.at;

        return Ok(Some(match reader.byte()? {
            FALSE => FieldRef::Bool(false),
            TRUE => FieldRef::Bool(true),
            INT => FieldRef::Int(unzigzag(reader.varint()?)),
            FLOAT => {
                let mut exact = [0u8; 8];

                exact.copy_from_slice(reader.take(8)?);
                FieldRef::Float(f64::from_le_bytes(exact))
            }
            STRING => {
                let len = reader.varint()?;

                FieldRef::String(
                    std::str::from_utf8(reader.take(len)?)
                        .map_err(|_| "a record holds a string that is not UTF-8")?,
                )
            }
            BYTES => {
                let len = reader.varint()?;

                FieldRef::Bytes(reader.take(len)?)
            }
            _ => {
                reader.at = start;
                reader.skip(0)?;
                FieldRef::Encoded(&bytes[start..reader.at])
            }
        }));
    }

    Ok(None)
}

/// The value whose encoding `bytes` [`find_field`] gave, as a field of kind
/// `kind`.
pub(crate) fn value_of(bytes: &[u8], kind: &Kind) -> Result<Value, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let raw = reader.value(0)?;

    if reader.at != bytes.len() {
        return Err("a record's value has bytes after its end");
    }

    from_raw(raw, kind)
}

/// A position in a record being read.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn byte(&mut self) -> Result<u8, &'static str> {
        let byte = *self.bytes.get(self.at).ok_or("a record ends early")?;

        self.at += 1;

        Ok(byte)
    }

    fn take(&mut self, len: u64) -> Result<&'a [u8], &'static str> {
        let len = usize::try_from(len).map_err(|_| "a record ends early")?;

        if len > self.left() {
            return Err("a record ends early");
        }

        let bytes: &'a [u8] = self.bytes;

        self.at += len;

        Ok(&bytes[self.at - len..self.at])
    }

    fn varint(&mut self) -> Result<u64, &'static str> {
        let mut value = 0u64;

        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7F);

            if shift == 63 && bits > 1 {
                return Err("a record has a number too large");
            }

            value |= bits << shift;

            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }

        Err("a record has a number too long")
    }

    /// A float's eight bytes, after its tag.
    fn float(&mut self) -> Result<f64, &'static str> {
        let mut exact = [0u8; 8];

        exact.copy_from_slice(self.take(8)?);

        Ok(f64::from_le_bytes(exact))
    }

    /// A string's length and bytes, after its tag.
    fn string(&mut self) -> Result<String, &'static str> {
        String::from_utf8(self.byte_string()?)
            .map_err(|_| "a record holds a string that is not UTF-8")
    }

    /// A byte string's length and bytes, after its tag.
    fn byte_string(&mut self) -> Result<Vec<u8>, &'static str> {
        let len = self.varint()?;

        Ok(self.take(len)?.to_vec())
    }

    /// A count of things each at least `each` bytes long, which a damaged
    /// record cannot make larger than what is left of it.
    fn count(&mut self, each: usize) -> Result<usize, &'static str> {
        let count = self.varint()?;

        usize::try_from(count)
            .ok()
            .filter(|count| count.saturating_mul(each) <= self.left())
            .ok_or("a record counts more than it holds")
    }

    /// The fields of a record under `fields`, as [`object_of`] reads them.
    fn object(&mut self, fields: &Fields, depth: usize) -> Result<Object, &'static str> {
        let count = self.count(2)?;
        let mut object = Vec::with_capacity(fields.list.len());
        let mut schema = fields.list.iter().peekable();
        let mut last = None;

        for _ in 0..count {
            let id = self.varint()?;

            if last.is_some_and(|last| last >= id) {
                return Err("a record's field ids are out of order");
            }

            last = Some(id);

            while let Some(field) = schema.next_if(|field| field.id < id) {
                object.push((Name::from(field.name.as_str()), absent(field)?));
            }

            match schema.next_if(|field| field.id == id) {
                Some(field) => {
                    let value = self.value_as(&field.kind, depth)?;

                    object.push((Name::from(field.name.as_str()), value));
                }
                // A field the schema no longer has, read to be checked like
                // the rest of the record.
                None => {
                    self.value(depth)?;
                }
            }
        }

        for field in schema {
            object.push((Name::from(field.name.as_str()), absent(field)?));
        }

        Ok(Object::from_fields(object))
    }

    /// A value of kind `kind`, read straight into one where the kind is a
    /// scalar or an embedded object, as [`from_raw`] would make it.
    fn value_as(&mut self, kind: &Kind, depth: usize) -> Result<Value, &'static str> {
        if depth >= MAX_DEPTH {
            return Err("a record nests too deeply");
        }

        let start = self.at;

        match (kind, self.byte()?) {
            (Kind::Bool, FALSE) => Ok(Value::Bool(false)),
            (Kind::Bool, TRUE) => Ok(Value::Bool(true)),
            (Kind::Int, INT) => Ok(Value::Int(unzigzag(self.varint()?))),
            (Kind::Float, FLOAT) => self.float().map(Value::Float),
            (Kind::String, STRING) => self.string().map(Value::String),
            (Kind::Bytes, BYTES) => self.byte_string().map(Value::Bytes),
            (Kind::Link { .. }, LINK) if depth + 1 < MAX_DEPTH => match self.byte()? {
                INT => Ok(Value::Int(unzigzag(self.varint()?))),
                STRING => self.string().map(Value::String),
                BYTES => self.byte_string().map(Value::Bytes),
                _ => {
                    self.at = start;
                    from_raw(self.value(depth)?, kind)
                }
            },
            (Kind::Object(fields), OBJECT) => {
                let len = self.varint()?;
                let bytes = self.take(len)?;
                let mut inner = Reader { bytes, at: 0 };
                let object = inner.object(fields, depth + 1)?;

                if inner.at != bytes.len() {
                    return Err("an embedded record has bytes after its last field");
                }

                Ok(Value::Object(object))
            }
            _ => {
                self.at = start;
                from_raw(self.value(depth)?, kind)
            }
        }
    }

    fn fields(&mut self, depth: usize) -> Result<Vec<(u64, Raw)>, &'static str> {
        let count = self.count(2)?;
        let mut fields = Vec::with_capacity(count.min(RESERVE));

        for _ in 0..count {
            let id = self.varint()?;

            if fields.last().is_some_and(|(last, _)| *last >= id) {
                return Err("a record's field ids are out of order");
            }

            fields.push((id, self.value(depth)?));
        }

        Ok(fields)
    }

    /// Steps over a value without reading it into one.
    fn skip(&mut self, depth: usize) -> Result<(), &'static str> {
        if depth >= MAX_DEPTH {
            return Err("a record nests too deeply");
        }

        match self.byte()? {
            FALSE | TRUE => {}
            INT => {
                self.varint()?;
            }
            FLOAT => {
                self.take(8)?;
            }
            STRING | BYTES | OBJECT => {
                let len = self.varint()?;

                self.take(len)?;
            }
            LIST => {
                for _ in 0..self.count(1)? {
                    self.skip(depth + 1)?;
                }
            }
            LINK => self.skip(depth + 1)?,
            _ => return Err("a record has an unknown tag"),
        }

        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Raw, &'static str> {
        if depth >= MAX_DEPTH {
            return Err("a record nests too deeply");
        }

        match self.byte()? {
            FALSE => Ok(Raw::Bool(false)),
            TRUE => Ok(Raw::Bool(true)),
            INT => Ok(Raw::Int(unzigzag(self.varint()?))),
            FLOAT => self.float().map(Raw::Float),
            STRING => self.string().map(Raw::String),
            BYTES => self.byte_string().map(Raw::Bytes),
            LIST => {
                let count = self.count(1)?;
                let mut values = Vec::with_capacity(count.min(RESERVE));

                for _ in 0..count {
                    values.push(self.value(depth + 1)?);
                }

                Ok(Raw::List(values))
            }
            OBJECT => {
                let len = self.varint()?;
                let bytes = self.take(len)?;
                let mut inner = Reader { bytes, at: 0 };
                let fields = inner.fields(depth + 1)?;

                if inner.at != bytes.len() {
                    return Err("an embedded record has bytes after its last field");
                }

                Ok(Raw::Object(fields))
            }
            LINK => Ok(Raw::Link(Box::new(self.value(depth + 1)?))),
            _ => Err("a record has an unknown tag"),
        }
    }
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(value.to_le_bytes()[0] | 0x80);
        value >>= 7;
    }

    out.push(value.to_le_bytes()[0]);
}

fn zigzag(value: i64) -> u64 {
    u64::from_le_bytes(((value << 1) ^ (value >> 63)).to_le_bytes())
}

fn unzigzag(value: u64) -> i64 {
    i64::from_le_bytes((value >> 1).to_le_bytes()) ^ -i64::from_le_bytes((value & 1).to_le_bytes())
}

/// The primary key kinds of the collections links point to, by collection id.
pub(crate) type KeyKinds<'a> = &'a dyn Fn(u64) -> Option<Kind>;

/// The fields of `object` for a record, checked against `fields`: every name
/// is a field, every value has its field's type, and every required field
/// without a default has a value. A required field left out is written with
/// its default, so that a later change of the default changes no object
/// already written; an optional one left out or null is not written.
///
/// The error says what is wrong, for the caller to report as an invalid
/// argument.
pub(crate) fn from_object(
    object: &Object,
    fields: &Fields,
    keys: KeyKinds<'_>,
) -> Result<Vec<(u64, Raw)>, String> {
    if let Some((name, _)) = object
        .fields()
        .find(|(name, _)| fields.by_name(name).is_none())
    {
        return Err(format!("`{name}` is not a field"));
    }

    let mut raw = Vec::new();

    for field in &fields.list {
        let value = match object.get(&field.name) {
            None | Some(Value::Null) if field.optional => continue,
            None | Some(Value::Null) => match &field.default {
                Some(default) => default,
                None => return Err(format!("`{}` is required", field.name)),
            },
            Some(value) => value,
        };

        let converted = to_raw(value, &field.kind, keys)
            .map_err(|expected| format!("`{}` holds {expected}", field.name))?;

        raw.push((field.id, converted));
    }

    Ok(raw)
}

/// `value` as a record holds it, if it has kind `kind`. The error names the
/// kind expected.
fn to_raw(value: &Value, kind: &Kind, keys: KeyKinds<'_>) -> Result<Raw, String> {
    let mismatch = || format!("a value that is not {}", kind.describe());

    match (kind, value) {
        (Kind::Bool, Value::Bool(value)) => Ok(Raw::Bool(*value)),
        (Kind::Int, Value::Int(value)) => Ok(Raw::Int(*value)),
        (Kind::Float, Value::Float(value)) => Ok(Raw::Float(*value)),
        (Kind::String, Value::String(value)) => Ok(Raw::String(value.clone())),
        (Kind::Bytes, Value::Bytes(value)) => Ok(Raw::Bytes(value.clone())),
        (Kind::Link { collection }, value) => {
            let key = keys(*collection).ok_or_else(mismatch)?;

            Ok(Raw::Link(Box::new(to_raw(value, &key, keys)?)))
        }
        (Kind::List(element), Value::List(values)) => values
            .iter()
            .map(|value| to_raw(value, element, keys))
            .collect::<Result<_, _>>()
            .map(Raw::List),
        (Kind::Object(fields), Value::Object(object)) => {
            from_object(object, fields, keys).map(Raw::Object)
        }
        _ => Err(mismatch()),
    }
}

/// The object whose record fields are `raw`, under `fields`: a field absent
/// from the record holds its default or null, and a field id `fields` does not
/// have is skipped. A value whose tag does not fit its field's kind, or a
/// required field without a default missing, makes the record damaged.
pub(crate) fn to_object(raw: Vec<(u64, Raw)>, fields: &Fields) -> Result<Object, &'static str> {
    let mut object = Vec::with_capacity(fields.list.len());
    let mut raw = raw.into_iter().peekable();

    for field in &fields.list {
        while raw.next_if(|(id, _)| *id < field.id).is_some() {}

        let value = match raw.next_if(|(id, _)| *id == field.id) {
            Some((_, value)) => from_raw(value, &field.kind)?,
            None => match &field.default {
                Some(default) => default.clone(),
                None if field.optional => Value::Null,
                None => return Err("a record lacks a required field"),
            },
        };

        object.push((Name::from(field.name.as_str()), value));
    }

    Ok(Object::from_fields(object))
}

/// The fields a record from outside the engine holds, as an object with only
/// those fields: one a language binding sends to be written, which the write
/// then checks against the schema and fills in like any other object. A field
/// id `fields` does not have, or a value whose tag does not fit its field, is
/// refused.
pub(crate) fn to_partial_object(
    raw: Vec<(u64, Raw)>,
    fields: &Fields,
) -> Result<Object, &'static str> {
    let mut object = Vec::with_capacity(raw.len());

    // A record's field ids are in order and different, and so are the names
    // of the fields they belong to.
    for (id, value) in raw {
        let field = fields
            .by_id(id)
            .ok_or("a record holds a field id its collection does not have")?;

        object.push((
            Name::from(field.name.as_str()),
            from_raw(value, &field.kind)?,
        ));
    }

    Ok(Object::from_fields(object))
}

fn from_raw(raw: Raw, kind: &Kind) -> Result<Value, &'static str> {
    match (kind, raw) {
        (Kind::Bool, Raw::Bool(value)) => Ok(Value::Bool(value)),
        (Kind::Int, Raw::Int(value)) => Ok(Value::Int(value)),
        (Kind::Float, Raw::Float(value)) => Ok(Value::Float(value)),
        (Kind::String, Raw::String(value)) => Ok(Value::String(value)),
        (Kind::Bytes, Raw::Bytes(value)) => Ok(Value::Bytes(value)),
        (Kind::Link { .. }, Raw::Link(key)) => match *key {
            Raw::Int(value) => Ok(Value::Int(value)),
            Raw::String(value) => Ok(Value::String(value)),
            Raw::Bytes(value) => Ok(Value::Bytes(value)),
            _ => Err("a record links by a key that cannot be one"),
        },
        (Kind::List(element), Raw::List(values)) => values
            .into_iter()
            .map(|value| from_raw(value, element))
            .collect::<Result<_, _>>()
            .map(Value::List),
        (Kind::Object(fields), Raw::Object(raw)) => to_object(raw, fields).map(Value::Object),
        _ => Err("a record holds a value of another type than its field"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::schema::FieldDef;
    use super::*;
    use crate::testing::Rng;

    fn field(id: u64, name: &str, kind: Kind, optional: bool) -> FieldDef {
        FieldDef {
            id,
            name: name.to_owned(),
            kind,
            optional,
            default: None,
        }
    }

    fn fields() -> Fields {
        let address = Fields {
            list: vec![
                field(1, "city", Kind::String, false),
                field(2, "zip", Kind::Int, true),
            ],
            next_id: 3,
        };
        let mut age = field(4, "age", Kind::Int, false);

        age.default = Some(Value::Int(0));

        Fields {
            list: vec![
                field(1, "id", Kind::Int, false),
                field(2, "name", Kind::String, false),
                field(3, "email", Kind::String, true),
                age,
                field(5, "score", Kind::Float, true),
                field(6, "tags", Kind::List(Box::new(Kind::String)), true),
                field(7, "address", Kind::Object(address), true),
                field(8, "friend", Kind::Link { collection: 1 }, true),
                field(9, "photo", Kind::Bytes, true),
                field(10, "admin", Kind::Bool, true),
            ],
            next_id: 11,
        }
    }

    fn keys(collection: u64) -> Option<Kind> {
        (collection == 1).then_some(Kind::Int)
    }

    fn full() -> Object {
        Object::new()
            .with("id", 7)
            .with("name", "Ada")
            .with("email", Value::Null)
            .with("age", 36)
            .with("score", -0.5)
            .with("tags", vec![Value::from("a"), Value::from("b")])
            .with(
                "address",
                Object::new().with("city", "Seoul").with("zip", Value::Null),
            )
            .with("friend", 3)
            .with("photo", vec![0u8, 1, 255])
            .with("admin", true)
    }

    #[test]
    fn an_object_reads_back_as_it_was_written() {
        let fields = fields();
        let bytes = write(&from_object(&full(), &fields, &keys).unwrap());
        let read = to_object(read(&bytes).unwrap(), &fields).unwrap();

        assert_eq!(read, full());
        assert_eq!(object_of(&bytes, &fields).unwrap(), full());
    }

    #[test]
    fn left_out_fields_read_as_their_default_or_null() {
        let fields = fields();
        let written = Object::new().with("id", 1).with("name", "B");
        let bytes = write(&from_object(&written, &fields, &keys).unwrap());
        let read = to_object(super::read(&bytes).unwrap(), &fields).unwrap();

        assert_eq!(object_of(&bytes, &fields).unwrap(), read);

        assert_eq!(read.get("age"), Some(&Value::Int(0)));
        assert_eq!(read.get("email"), Some(&Value::Null));
        assert_eq!(read.len(), fields.list.len());
    }

    #[test]
    fn objects_that_do_not_fit_the_schema_are_refused() {
        let fields = fields();
        let cases = [
            full().with("nickname", "x"),
            full().with("age", "old"),
            full().with("friend", "not an int key"),
            full().with("tags", vec![Value::Int(1)]),
            full().with("address", Object::new()),
            {
                let mut object = full();

                object.remove("name");
                object
            },
        ];

        for object in cases {
            assert!(from_object(&object, &fields, &keys).is_err(), "{object:?}");
        }
    }

    #[test]
    fn a_field_the_schema_no_longer_has_is_skipped() {
        let fields = fields();
        let mut raw = from_object(&full(), &fields, &keys).unwrap();

        // A field removed from the schema, and one past every field it has.
        raw.push((99, Raw::Bytes(vec![9; 4])));

        let mut fewer = fields.clone();

        fewer.list.retain(|field| field.id != 2);

        let read = to_object(read(&write(&raw)).unwrap(), &fewer).unwrap();

        assert_eq!(object_of(&write(&raw), &fewer).unwrap(), read);
        assert!(read.get("name").is_none());
        assert_eq!(read.get("id"), Some(&Value::Int(7)));
        assert_eq!(read.get("admin"), Some(&Value::Bool(true)));
    }

    #[test]
    fn the_documented_layout_is_the_one_written() {
        let bytes = write(&[(1, Raw::Int(-2)), (3, Raw::String("hi".into()))]);

        assert_eq!(bytes, [2, 1, 0x04, 3, 3, 0x06, 2, b'h', b'i']);
        assert_eq!(write(&[(300, Raw::Bool(true))]), [1, 0xAC, 0x02, 0x03]);
    }

    #[test]
    fn integers_survive_the_zigzag() {
        for value in [0, 1, -1, 63, -64, i64::MAX, i64::MIN] {
            assert_eq!(unzigzag(zigzag(value)), value);
        }
    }

    #[test]
    fn random_bytes_read_as_a_record_or_as_damage_and_never_panic() {
        let mut rng = Rng::new(6);
        let fields = fields();
        let valid = write(&from_object(&full(), &fields, &keys).unwrap());

        for _ in 0..20_000 {
            let mut bytes = if rng.below(2) == 0 {
                valid.clone()
            } else {
                let len = rng.index(40);

                rng.bytes(len)
            };

            if !bytes.is_empty() {
                for _ in 0..1 + rng.index(3) {
                    let at = rng.index(bytes.len());

                    bytes[at] = rng.next_u64().to_le_bytes()[0];
                }
            }

            let staged = read(&bytes).and_then(|raw| to_object(raw, &fields));
            let direct = object_of(&bytes, &fields);

            match (staged, direct) {
                (Ok(staged), Ok(direct)) => assert_eq!(staged, direct, "{bytes:?}"),
                (Err(_), Err(_)) => {}
                (staged, direct) => {
                    panic!("{bytes:?}: {staged:?} read in two steps, {direct:?} at once")
                }
            }
        }

        let mut nested = vec![1, 1];

        for _ in 0..200 {
            nested.extend_from_slice(&[LIST, 1]);
        }

        assert!(read(&nested).is_err(), "nesting past the limit");
    }
}
