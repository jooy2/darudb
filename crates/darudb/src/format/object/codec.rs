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

/// The order of the fields of `Fields` by name: `ranked[rank]` is the
/// position in the list of the field whose name comes `rank`th in byte
/// order.
///
/// An object keeps its fields sorted by name, and a record holds them by
/// id, so each object read was sorted. A caller that reads many objects of
/// one collection works the order out once, with this, and each object read
/// with [`object_in_order`] has its fields put in their places rather than
/// compared. The order is worked out from the same fields the objects are
/// read with, and kept no longer than the caller keeps them.
#[derive(Debug)]
pub(crate) struct NameOrder {
    ranked: Vec<usize>,
}

impl NameOrder {
    pub(crate) fn of(fields: &Fields) -> Self {
        let mut ranked: Vec<usize> = (0..fields.list.len()).collect();

        ranked.sort_unstable_by(|&a, &b| {
            fields.list[a]
                .name
                .as_bytes()
                .cmp(fields.list[b].name.as_bytes())
        });

        Self { ranked }
    }
}

/// [`object_of`], with the fields put in name order by `order`, which was
/// worked out from `fields`.
pub(crate) fn object_in_order(
    bytes: &[u8],
    fields: &Fields,
    order: &NameOrder,
) -> Result<Object, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let mut object = reader.object_fields(fields, 0)?;

    if reader.at != bytes.len() {
        return Err("a record has bytes after its last field");
    }

    Ok(if rank(&mut object, &order.ranked) {
        Object::from_sorted(object)
    } else {
        Object::from_fields(object)
    })
}

/// Moves `fields`, in the order of their schema's list, to the places
/// `ranked` gives them, by swapping them along the cycles of the order.
/// Returns false, having moved nothing, when it cannot: the order is not
/// one of as many fields, or there are more than 64 of them.
fn rank(fields: &mut [(Name, Value)], ranked: &[usize]) -> bool {
    if ranked.len() != fields.len() || fields.len() > 64 {
        return false;
    }

    // `placed` has a bit for each position whose field is where it goes.
    let mut placed = 0u64;

    for start in 0..fields.len() {
        let mut at = start;

        while placed & (1 << at) == 0 {
            placed |= 1 << at;

            let from = ranked[at];

            if from == start {
                break;
            }

            fields.swap(at, from);
            at = from;
        }
    }

    true
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
    /// A string's bytes, checked to be UTF-8 as [`utf8`] checks them.
    String(&'a [u8]),
    Bytes(&'a [u8]),
    Encoded(&'a [u8]),
}

/// Whether `bytes` are UTF-8. Bytes that are all below `0x80` are, and
/// telling so is a short loop the compiler puts in place; any others go
/// through the full check. A filter reads a string field of every record it
/// tests, and the call the full check costs took a fifth of the time of a
/// filter on a short string.
#[inline]
pub(crate) fn utf8(bytes: &[u8]) -> bool {
    ascii(bytes) || std::str::from_utf8(bytes).is_ok()
}

/// Whether every byte of `bytes` is below `0x80`, eight bytes at a time, in
/// the caller. The standard library's check is a call of its own, which cost
/// a filter on a short string field an eighth of its time.
#[inline(always)]
fn ascii(bytes: &[u8]) -> bool {
    let mut chunks = bytes.chunks_exact(8);
    let mut high = 0u64;

    for chunk in &mut chunks {
        let mut word = [0u8; 8];

        word.copy_from_slice(chunk);
        high |= u64::from_le_bytes(word);
    }

    for byte in chunks.remainder() {
        high |= u64::from(*byte);
    }

    high & 0x8080_8080_8080_8080 == 0
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
            reader.skip_scalar()?;

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
                let text = reader.take(len)?;

                if !utf8(text) {
                    return Err("a record holds a string that is not UTF-8");
                }

                FieldRef::String(text)
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

/// The value of a field of kind `kind` that [`find_field`] found, as
/// [`object_of`] would read it: a record whose value does not fit the kind
/// is damaged.
pub(crate) fn field_value(found: FieldRef<'_>, kind: &Kind) -> Result<Value, &'static str> {
    match (found, kind) {
        (FieldRef::Bool(value), Kind::Bool) => Ok(Value::Bool(value)),
        (FieldRef::Int(value), Kind::Int) => Ok(Value::Int(value)),
        (FieldRef::Float(value), Kind::Float) => Ok(Value::Float(value)),
        (FieldRef::String(text), Kind::String) => String::from_utf8(text.to_vec())
            .map(Value::String)
            .map_err(|_| "a record holds a string that is not UTF-8"),
        (FieldRef::Bytes(bytes), Kind::Bytes) => Ok(Value::Bytes(bytes.to_vec())),
        (FieldRef::Encoded(bytes), kind) => value_of(bytes, kind),
        _ => Err("a record holds a value of another type than its field"),
    }
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
        self.object_fields(fields, depth).map(Object::from_fields)
    }

    /// The fields of an object, one for each field of `fields` and in the
    /// order of their list, a field the record leaves out holding its
    /// default.
    fn object_fields(
        &mut self,
        fields: &Fields,
        depth: usize,
    ) -> Result<Vec<(Name, Value)>, &'static str> {
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

        Ok(object)
    }

    /// A value of the scalar kind `kind`, borrowed from the record: what
    /// [`value_as`](Self::value_as) reads, and refuses, without copying a
    /// string or bytes.
    fn scalar_as(&mut self, kind: &Kind) -> Result<FieldRef<'a>, &'static str> {
        let start = self.at;

        match (kind, self.byte()?) {
            (Kind::Bool, FALSE) => Ok(FieldRef::Bool(false)),
            (Kind::Bool, TRUE) => Ok(FieldRef::Bool(true)),
            (Kind::Int, INT) => Ok(FieldRef::Int(unzigzag(self.varint()?))),
            (Kind::Float, FLOAT) => self.float().map(FieldRef::Float),
            (Kind::String, STRING) => {
                let len = self.varint()?;
                let text = self.take(len)?;

                if !utf8(text) {
                    return Err("a record holds a string that is not UTF-8");
                }

                Ok(FieldRef::String(text))
            }
            (Kind::Bytes, BYTES) => {
                let len = self.varint()?;

                self.take(len).map(FieldRef::Bytes)
            }
            // A value of another type: refused as `value_as` refuses it,
            // after reading it whole, which may find it damaged first.
            _ => {
                self.at = start;
                from_raw(self.value(0)?, kind)?;

                Err("a record holds a value of another type than its field")
            }
        }
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

    /// [`skip`](Self::skip), with a scalar stepped over in the caller: most
    /// fields a filter steps over on its way to the one it tests are
    /// scalars, and a call for each cost more than stepping over it.
    #[inline(always)]
    fn skip_scalar(&mut self) -> Result<(), &'static str> {
        match self.bytes.get(self.at) {
            Some(&(FALSE | TRUE)) => {
                self.at += 1;
            }
            Some(&INT) => {
                self.at += 1;
                self.varint()?;
            }
            Some(&FLOAT) => {
                self.at += 1;
                self.take(8)?;
            }
            Some(&(STRING | BYTES)) => {
                self.at += 1;

                let len = self.varint()?;

                self.take(len)?;
            }
            _ => self.skip(0)?,
        }

        Ok(())
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

/// The varint of `value`, in the first of the bytes, and how many it takes.
fn varint_bytes(mut value: u64) -> ([u8; 10], usize) {
    let mut bytes = [0; 10];
    let mut len = 0;

    while value >= 0x80 {
        bytes[len] = value.to_le_bytes()[0] | 0x80;
        value >>= 7;
        len += 1;
    }

    bytes[len] = value.to_le_bytes()[0];

    (bytes, len + 1)
}

fn zigzag(value: i64) -> u64 {
    u64::from_le_bytes(((value << 1) ^ (value >> 63)).to_le_bytes())
}

fn unzigzag(value: u64) -> i64 {
    i64::from_le_bytes((value >> 1).to_le_bytes()) ^ -i64::from_le_bytes((value & 1).to_le_bytes())
}

/// The primary key kinds of the collections links point to, by collection id.
pub(crate) type KeyKinds<'a> = &'a dyn Fn(u64) -> Option<Kind>;

/// The record of `object`, checked against `fields`: every name is a field,
/// every value has its field's type, and every required field without a
/// default has a value. A required field left out is written with its
/// default, so that a later change of the default changes no object already
/// written; an optional one left out or null is not written.
///
/// The object is written straight into the record's bytes. Going through
/// [`Raw`] values first copied every string and byte value of the object, and
/// grew one vector for the values and another for the bytes.
///
/// The error says what is wrong, for the caller to report as an invalid
/// argument. A name that is not a field is reported before anything else
/// wrong with the object.
pub(crate) fn record_of(
    object: &Object,
    fields: &Fields,
    keys: KeyKinds<'_>,
) -> Result<Vec<u8>, String> {
    // Room for the fields of a small object, which most are, so that the
    // record is written without growing its buffer.
    let mut out = Vec::with_capacity(16 * (fields.list.len() + 1));

    encode_object(object, fields, keys, &mut out)?;

    Ok(out)
}

fn encode_object(
    object: &Object,
    fields: &Fields,
    keys: KeyKinds<'_>,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    let start = out.len();
    let most = fields.list.len() as u64;

    // The count of fields comes first, and is known once they are written:
    // room is left for the most it can be, and taken back if it is shorter.
    write_varint(most, out);

    let reserved = out.len() - start;
    let mut known = 0;
    let written = encode_fields(object, fields, keys, out, &mut known);

    if written.is_err() || known != object.len() {
        if let Some((name, _)) = object
            .fields()
            .find(|(name, _)| fields.by_name(name).is_none())
        {
            return Err(format!("`{name}` is not a field"));
        }
    }

    let count = written?;

    if count != most {
        let (prefix, len) = varint_bytes(count);

        if len == reserved {
            out[start..start + len].copy_from_slice(&prefix[..len]);
        } else {
            out.splice(start..start + reserved, prefix[..len].iter().copied());
        }
    }

    Ok(())
}

/// Writes the fields of `object` that `fields` has, by id, and returns how
/// many it wrote. `known` counts the object's fields that `fields` has, for
/// the caller to tell whether the object has others.
fn encode_fields(
    object: &Object,
    fields: &Fields,
    keys: KeyKinds<'_>,
    out: &mut Vec<u8>,
    known: &mut usize,
) -> Result<u64, String> {
    let mut count = 0;

    for field in &fields.list {
        let value = object.get(&field.name);

        if value.is_some() {
            *known += 1;
        }

        let value = match value {
            None | Some(Value::Null) if field.optional => continue,
            None | Some(Value::Null) => match &field.default {
                Some(default) => default,
                None => return Err(format!("`{}` is required", field.name)),
            },
            Some(value) => value,
        };

        write_varint(field.id, out);
        encode_value(value, &field.kind, keys, out)
            .map_err(|expected| format!("`{}` holds {expected}", field.name))?;
        count += 1;
    }

    Ok(count)
}

/// Writes `value` as a record holds it, if it has kind `kind`. The error
/// names the kind expected.
fn encode_value(
    value: &Value,
    kind: &Kind,
    keys: KeyKinds<'_>,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    let mismatch = || format!("a value that is not {}", kind.describe());

    match (kind, value) {
        (Kind::Bool, Value::Bool(false)) => out.push(FALSE),
        (Kind::Bool, Value::Bool(true)) => out.push(TRUE),
        (Kind::Int, Value::Int(value)) => {
            out.push(INT);
            write_varint(zigzag(*value), out);
        }
        (Kind::Float, Value::Float(value)) => {
            out.push(FLOAT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        (Kind::String, Value::String(value)) => {
            out.push(STRING);
            write_varint(value.len() as u64, out);
            out.extend_from_slice(value.as_bytes());
        }
        (Kind::Bytes, Value::Bytes(value)) => {
            out.push(BYTES);
            write_varint(value.len() as u64, out);
            out.extend_from_slice(value);
        }
        (Kind::Link { collection }, value) => {
            let key = keys(*collection).ok_or_else(mismatch)?;

            out.push(LINK);
            encode_value(value, &key, keys, out)?;
        }
        (Kind::List(element), Value::List(values)) => {
            out.push(LIST);
            write_varint(values.len() as u64, out);

            for value in values {
                encode_value(value, element, keys, out)?;
            }
        }
        (Kind::Object(fields), Value::Object(object)) => {
            // An embedded record is preceded by its length, known once it is
            // written, so it is written apart first.
            let mut inner = Vec::new();

            encode_object(object, fields, keys, &mut inner)?;
            out.push(OBJECT);
            write_varint(inner.len() as u64, out);
            out.extend_from_slice(&inner);
        }
        _ => return Err(mismatch()),
    }

    Ok(())
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

/// The fields the record `bytes` from outside the engine holds, as an
/// object with only those fields: one a language binding sends to be
/// written, which the write then checks against the schema and fills in like
/// any other object. A field id `fields` does not have, or a value whose tag
/// does not fit its field, is refused, as are bytes that are not one record.
///
/// The values are read straight from the record, as [`object_of`] reads
/// them, rather than into [`Raw`] values first. A record with more than one
/// thing wrong is refused for the first found, which may not be the one
/// reading it whole first would name.
pub(crate) fn partial_object_of(bytes: &[u8], fields: &Fields) -> Result<Object, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let count = reader.count(2)?;
    let mut object = Vec::with_capacity(count.min(RESERVE));
    let mut last = None;

    for _ in 0..count {
        let id = reader.varint()?;

        if last.is_some_and(|last| last >= id) {
            return Err("a record's field ids are out of order");
        }

        last = Some(id);

        // A record's field ids are in order and different, and so are the
        // names of the fields they belong to.
        let field = fields
            .by_id(id)
            .ok_or("a record holds a field id its collection does not have")?;

        object.push((
            Name::from(field.name.as_str()),
            reader.value_as(&field.kind, 0)?,
        ));
    }

    if reader.at != bytes.len() {
        return Err("a record has bytes after its last field");
    }

    Ok(Object::from_fields(object))
}

/// Whether every field of `fields` holds a scalar: a bool, an int, a float,
/// a string or bytes. The records a binding sends for such a collection are
/// checked and completed as they are, by [`flat_fields`] and
/// [`flat_record`], rather than read into an object and written again.
pub(crate) fn is_flat(fields: &Fields) -> bool {
    fields.list.iter().all(|field| {
        matches!(
            field.kind,
            Kind::Bool | Kind::Int | Kind::Float | Kind::String | Kind::Bytes
        )
    })
}

/// The fields the record `bytes` from outside the engine holds, under
/// `fields`, which [`is_flat`] accepts: each as the position of its field in
/// the list and its value, borrowed from the record. The record is checked as
/// [`partial_object_of`] checks it, and refused for the same reasons, in the
/// same order.
pub(crate) fn flat_fields<'a>(
    bytes: &'a [u8],
    fields: &Fields,
) -> Result<Vec<(usize, FieldRef<'a>)>, &'static str> {
    let mut reader = Reader { bytes, at: 0 };
    let count = reader.count(2)?;
    // Ids in order and all of them the schema's: no more than it has.
    let mut present = Vec::with_capacity(count.min(fields.list.len()));
    let mut last = None;

    for _ in 0..count {
        let id = reader.varint()?;

        if last.is_some_and(|last| last >= id) {
            return Err("a record's field ids are out of order");
        }

        last = Some(id);

        let position = fields
            .list
            .iter()
            .position(|field| field.id == id)
            .ok_or("a record holds a field id its collection does not have")?;

        present.push((position, reader.scalar_as(&fields.list[position].kind)?));
    }

    if reader.at != bytes.len() {
        return Err("a record has bytes after its last field");
    }

    Ok(present)
}

/// The record the file holds for the fields `present` that [`flat_fields`]
/// read, under the same `fields`: the bytes [`record_of`] writes for the
/// object they make. A required field left out is written with its default,
/// and an optional one is not written. `assigned` is the position in the
/// list of the auto-increment key the record left out, and the number it
/// gets.
///
/// The error says what is wrong, as [`record_of`] says it.
pub(crate) fn flat_record(
    present: &[(usize, FieldRef<'_>)],
    fields: &Fields,
    assigned: Option<(usize, i64)>,
) -> Result<Vec<u8>, String> {
    let given = |position: usize| {
        present
            .iter()
            .find(|(at, _)| *at == position)
            .map(|(_, value)| *value)
            .or_else(|| {
                assigned
                    .filter(|(at, _)| *at == position)
                    .map(|(_, number)| FieldRef::Int(number))
            })
    };
    let mut count = 0u64;

    // The count comes first: work it out, and refuse a required field left
    // out, before writing anything.
    for (position, field) in fields.list.iter().enumerate() {
        if given(position).is_some() || (!field.optional && field.default.is_some()) {
            count += 1;
        } else if !field.optional {
            return Err(format!("`{}` is required", field.name));
        }
    }

    let mut out = Vec::with_capacity(16 * (fields.list.len() + 1));

    write_varint(count, &mut out);

    for (position, field) in fields.list.iter().enumerate() {
        match (given(position), &field.default) {
            (Some(value), _) => {
                write_varint(field.id, &mut out);
                write_ref(value, &mut out);
            }
            (None, Some(default)) if !field.optional => {
                write_varint(field.id, &mut out);
                encode_value(default, &field.kind, &|_| None, &mut out)
                    .map_err(|expected| format!("`{}` holds {expected}", field.name))?;
            }
            (None, _) => {}
        }
    }

    Ok(out)
}

/// Writes a value [`find_field`] or [`flat_fields`] read, as a record holds
/// it.
fn write_ref(value: FieldRef<'_>, out: &mut Vec<u8>) {
    match value {
        FieldRef::Bool(false) => out.push(FALSE),
        FieldRef::Bool(true) => out.push(TRUE),
        FieldRef::Int(value) => {
            out.push(INT);
            write_varint(zigzag(value), out);
        }
        FieldRef::Float(value) => {
            out.push(FLOAT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        FieldRef::String(text) => {
            out.push(STRING);
            write_varint(text.len() as u64, out);
            out.extend_from_slice(text);
        }
        FieldRef::Bytes(bytes) => {
            out.push(BYTES);
            write_varint(bytes.len() as u64, out);
            out.extend_from_slice(bytes);
        }
        // Tag included.
        FieldRef::Encoded(bytes) => out.extend_from_slice(bytes),
    }
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

    /// Objects read with their fields put in name order by a `NameOrder`
    /// are the objects read and sorted, for schemas of every size up to
    /// past the 64 fields the order handles, fields left out of the record
    /// among them.
    #[test]
    fn an_object_read_in_a_worked_out_order_is_the_sorted_one() {
        let mut rng = Rng::new(11);

        for round in 0..400 {
            let count = round % 71;
            let mut names: Vec<String> = Vec::new();

            while names.len() < count {
                let name: String = (0..1 + rng.index(4))
                    .map(|_| char::from(b"abz_"[rng.index(4)]))
                    .collect();

                if !names.contains(&name) {
                    names.push(name);
                }
            }

            let fields = Fields {
                list: names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| field(at as u64 + 1, name, Kind::Int, true))
                    .collect(),
                next_id: count as u64 + 1,
            };
            let record = write(
                &(1..=count as u64)
                    .filter(|_| rng.below(3) != 0)
                    .map(|id| (id, Raw::Int(i64::try_from(id).unwrap() * 3)))
                    .collect::<Vec<_>>(),
            );
            let order = NameOrder::of(&fields);

            assert_eq!(
                object_in_order(&record, &fields, &order),
                object_of(&record, &fields),
                "{names:?}"
            );
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
        let bytes = record_of(&full(), &fields, &keys).unwrap();
        let read = to_object(read(&bytes).unwrap(), &fields).unwrap();

        assert_eq!(read, full());
        assert_eq!(object_of(&bytes, &fields).unwrap(), full());
    }

    #[test]
    fn left_out_fields_read_as_their_default_or_null() {
        let fields = fields();
        let written = Object::new().with("id", 1).with("name", "B");
        let bytes = record_of(&written, &fields, &keys).unwrap();
        let read = to_object(super::read(&bytes).unwrap(), &fields).unwrap();

        assert_eq!(object_of(&bytes, &fields).unwrap(), read);

        assert_eq!(read.get("age"), Some(&Value::Int(0)));
        assert_eq!(read.get("email"), Some(&Value::Null));
        assert_eq!(read.len(), fields.list.len());
    }

    /// The record written from an object is the one its values give, field
    /// by field in id order, with an optional field left out or null not
    /// written.
    #[test]
    fn an_object_is_written_as_its_values_give() {
        let expected = write(&[
            (1, Raw::Int(7)),
            (2, Raw::String("Ada".into())),
            (4, Raw::Int(36)),
            (5, Raw::Float(-0.5)),
            (
                6,
                Raw::List(vec![Raw::String("a".into()), Raw::String("b".into())]),
            ),
            (7, Raw::Object(vec![(1, Raw::String("Seoul".into()))])),
            (8, Raw::Link(Box::new(Raw::Int(3)))),
            (9, Raw::Bytes(vec![0, 1, 255])),
            (10, Raw::Bool(true)),
        ]);

        assert_eq!(record_of(&full(), &fields(), &keys).unwrap(), expected);
    }

    /// The count of fields leads the record, and is written in as many bytes
    /// as it takes, when the schema has so many fields that the most it could
    /// be takes more.
    #[test]
    fn the_count_of_fields_takes_the_bytes_it_needs() {
        let fields = Fields {
            list: (1..=200)
                .map(|id| field(id, &format!("f{id}"), Kind::Int, true))
                .collect(),
            next_id: 201,
        };

        for set in [0, 1, 127, 128, 200] {
            let object = Object::from_fields(
                (1..=set)
                    .map(|id| (Name::from(format!("f{id}").as_str()), Value::Int(id)))
                    .collect(),
            );
            let bytes = record_of(&object, &fields, &keys).unwrap();
            let expected = write(
                &(1..=set)
                    .map(|id| (id.cast_unsigned(), Raw::Int(id)))
                    .collect::<Vec<_>>(),
            );

            assert_eq!(bytes, expected, "{set} fields set");
            assert_eq!(object_of(&bytes, &fields).unwrap().len(), 200);
        }
    }

    /// The error names what is wrong, and a name that is not a field comes
    /// before anything else wrong with the object, in an embedded object
    /// too.
    #[test]
    fn a_refused_object_is_told_what_is_wrong() {
        let fields = fields();
        let mut missing = full();

        missing.remove("name");

        let cases = [
            (
                full().with("age", "old"),
                "`age` holds a value that is not an int",
            ),
            (missing.clone(), "`name` is required"),
            (missing.with("nickname", "x"), "`nickname` is not a field"),
            (
                full().with("age", "old").with("zzz", 1),
                "`zzz` is not a field",
            ),
            (
                full().with("friend", "x"),
                "`friend` holds a value that is not an int",
            ),
            (
                full().with("tags", vec![Value::Int(1)]),
                "`tags` holds a value that is not a string",
            ),
            (
                full().with("address", Object::new().with("zip", "x").with("zzz", 1)),
                "`address` holds `zzz` is not a field",
            ),
            (
                full().with("address", Object::new()),
                "`address` holds `city` is required",
            ),
        ];

        for (object, message) in cases {
            assert_eq!(
                record_of(&object, &fields, &keys),
                Err(message.to_owned()),
                "{object:?}"
            );
        }
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
            assert!(record_of(&object, &fields, &keys).is_err(), "{object:?}");
        }
    }

    #[test]
    fn a_field_the_schema_no_longer_has_is_skipped() {
        let fields = fields();
        let mut raw = read(&record_of(&full(), &fields, &keys).unwrap()).unwrap();

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

    /// A string field found for a filter is checked to be UTF-8, whether
    /// its bytes are all ASCII or not.
    #[test]
    fn a_string_found_for_a_filter_is_checked_to_be_utf8() {
        let record = write(&[
            (1, Raw::String("plain".into())),
            (2, Raw::String("\u{d55c}\u{ae00} \u{e9}".into())),
        ]);

        assert_eq!(find_field(&record, 1), Ok(Some(FieldRef::String(b"plain"))));
        assert_eq!(
            find_field(&record, 2),
            Ok(Some(FieldRef::String("\u{d55c}\u{ae00} \u{e9}".as_bytes())))
        );

        // A continuation byte alone, a byte UTF-8 never uses, and a sequence
        // cut short, each after ASCII.
        for bad in [&[b'a', 0x80][..], &[b'a', 0xFF], &[b'a', 0xE2, 0x82]] {
            let mut record = vec![1, 1, STRING, u8::try_from(bad.len()).unwrap()];

            record.extend_from_slice(bad);

            assert!(find_field(&record, 1).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn utf8_is_what_the_full_check_says() {
        let mut rng = Rng::new(12);

        for _ in 0..20_000 {
            // Mostly ASCII, with a byte above it now and then, so that both
            // ways through the check are taken.
            let bytes: Vec<u8> = (0..rng.index(24))
                .map(|_| {
                    let byte = rng.next_u64().to_le_bytes()[0];

                    if rng.below(4) == 0 { byte } else { byte & 0x7F }
                })
                .collect();

            assert_eq!(
                utf8(&bytes),
                std::str::from_utf8(&bytes).is_ok(),
                "{bytes:?}"
            );
        }
    }

    /// A record a binding sends reads as the object that reading it into
    /// values and then converting them field by field gives, and random
    /// bytes are refused by the one exactly when they are by the other.
    #[test]
    fn a_record_to_write_reads_as_its_values_converted_one_by_one() {
        let fields = fields();
        let converted = |bytes: &[u8]| -> Result<Object, &'static str> {
            let mut object = Vec::new();

            for (id, raw) in read(bytes)? {
                let field = fields.by_id(id).ok_or("unknown")?;

                object.push((Name::from(field.name.as_str()), from_raw(raw, &field.kind)?));
            }

            Ok(Object::from_fields(object))
        };
        let mut rng = Rng::new(13);
        let valid = record_of(&full(), &fields, &keys).unwrap();

        assert_eq!(partial_object_of(&valid, &fields), converted(&valid));
        let mut written = full();

        // An optional field left null is not written, so the record lacks it.
        written.remove("email");
        assert_eq!(partial_object_of(&valid, &fields).unwrap(), written);

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

            match (partial_object_of(&bytes, &fields), converted(&bytes)) {
                (Ok(direct), Ok(staged)) => assert_eq!(direct, staged, "{bytes:?}"),
                (Err(_), Err(_)) => {}
                (direct, staged) => {
                    panic!("{bytes:?}: {direct:?} read at once, {staged:?} in two steps")
                }
            }
        }
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
        let valid = record_of(&full(), &fields, &keys).unwrap();

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
