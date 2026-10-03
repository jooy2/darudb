//! Objects as the application's own Rust types: [`CollectionType`] for a type
//! whose values are the objects of a collection, [`EmbeddedType`] for an
//! embedded object, [`FieldType`] for the type of a field, and the typed
//! collections a transaction gives for them. `#[derive(Object)]` and
//! `#[derive(Embedded)]`, with the `derive` feature, implement the first two.
//!
//! A typed read decodes a record straight into the type, with no [`Object`]
//! in between: building an `Object`, a vector of named values with an
//! allocation for each string, cost about as much as finding the record.
//! A typed write encodes the type into a record, which the engine checks as
//! it checks a record a language binding sends, so a hand-written
//! implementation cannot store what the schema does not allow.
//!
//! The type's fields are matched with the stored collection's by name once
//! for each handle and type, into a [`Layout`]: the stored fields in id
//! order, each with the number the type gives it, its slot, the bytes of its
//! default, and the layout of an embedded object. Reading walks the record
//! and the layout together; writing goes down the layout, so the fields of
//! the record come out in id order without being sorted.
//!
//! [`Object`]: crate::Object

use std::any::{Any, TypeId};
use std::fmt;
use std::marker::PhantomData;
use std::ops::Bound;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::format::object::Value;
use crate::format::object::codec::{
    self, BYTES, FALSE, FLOAT, INT, LINK, LIST, OBJECT, STRING, TRUE,
};
use crate::format::object::schema::{Fields, Kind, OpenSchema, StoredSchema};
use crate::txn::{ReadTransaction, WriteTransaction};

use super::declare::{Collection, Embedded, Field, Type};
use super::objects::{
    CollectionReader, CollectionWriter, Source, checked_schema, position, records,
};

/// A Rust type whose values are the objects of a collection.
///
/// `#[derive(Object)]` implements it; see the crate's documentation. A
/// hand-written implementation numbers the fields as
/// [`collection`](Self::collection) declares them, the auto-increment `id`
/// first when the collection has one, and that number is the `slot` the
/// other two methods take and give.
pub trait CollectionType: Sized + 'static {
    /// The type of the primary key: `i64` for an auto-increment.
    type Key: KeyType;

    /// The collection's name, the one [`collection`](Self::collection)
    /// declares. A link to the collection names it without declaring the
    /// collection, which a link to an object of its own collection would do
    /// without end.
    const COLLECTION: &'static str;

    /// The collection, as a schema declares it.
    fn collection() -> Collection;

    /// Writes the value of field `slot` into `value`.
    ///
    /// # Errors
    ///
    /// What writing the value returns.
    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()>;

    /// Reads an object from its fields, which `fields` gives once each.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a record that does not read as the type.
    fn read(fields: FieldReader<'_>) -> Result<Self>;
}

/// A Rust type whose values are embedded objects: fields of their own,
/// stored inside the object that holds them.
///
/// `#[derive(Embedded)]` implements it, together with [`FieldType`]. The
/// fields are numbered as [`embedded`](Self::embedded) declares them.
pub trait EmbeddedType: Sized + 'static {
    /// The embedded object's fields, as a schema declares them.
    fn embedded() -> Embedded;

    /// Writes the value of field `slot` into `value`.
    ///
    /// # Errors
    ///
    /// What writing the value returns.
    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()>;

    /// Reads an embedded object from its fields.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a record that does not read as the type.
    fn read(fields: FieldReader<'_>) -> Result<Self>;
}

/// The Rust type of a field: how it is declared, written and read.
///
/// The crate implements it for `bool`, `i64`, `f64`, `String`, `Vec<u8>`
/// (bytes), `Option<T>` (an optional field), `Vec<T>` (a list, of an
/// [`ElementType`]) and [`Link<T>`]; `#[derive(Embedded)]` implements it for
/// an embedded object.
pub trait FieldType: Sized {
    /// Whether the field may be null, as an `Option` may.
    const OPTIONAL: bool = false;

    /// The field's type in a schema.
    fn kind() -> Type;

    /// Writes the value.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a value the field cannot hold.
    fn write(&self, value: ValueWriter<'_>) -> Result<()>;

    /// Reads the value.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type.
    fn read(value: ValueReader<'_>) -> Result<Self>;
}

/// A type a list may hold: a scalar or a link.
pub trait ElementType: FieldType {}

/// A type a primary key may have: `i64`, `String` or `Vec<u8>`.
pub trait KeyType: FieldType {
    /// The key as a [`Value`], for a lookup.
    fn to_value(&self) -> Value;

    /// The key a write returned.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a value of another type.
    fn from_value(value: Value) -> Result<Self>;
}

/// A link to an object of `T`'s collection: the object's primary key.
///
/// A link to an object that does not exist, or no longer does, is allowed,
/// and reads as the key it holds.
pub struct Link<T: CollectionType> {
    key: T::Key,
    target: PhantomData<fn() -> T>,
}

impl<T: CollectionType> Link<T> {
    /// A link to the object whose primary key is `key`.
    pub fn new(key: T::Key) -> Self {
        Self {
            key,
            target: PhantomData,
        }
    }

    /// The primary key of the object linked to.
    pub fn key(&self) -> &T::Key {
        &self.key
    }

    /// The primary key, taken out of the link.
    pub fn into_key(self) -> T::Key {
        self.key
    }
}

// Written out rather than derived, which would ask `T` itself for each trait
// rather than its key.
impl<T: CollectionType> Clone for Link<T>
where
    T::Key: Clone,
{
    fn clone(&self) -> Self {
        Self::new(self.key.clone())
    }
}

impl<T: CollectionType> PartialEq for Link<T>
where
    T::Key: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: CollectionType> Eq for Link<T> where T::Key: Eq {}

impl<T: CollectionType> std::hash::Hash for Link<T>
where
    T::Key: std::hash::Hash,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl<T: CollectionType> fmt::Debug for Link<T>
where
    T::Key: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Link").field(&self.key).finish()
    }
}

/// The error for a value a record holds that its field's type does not.
fn wrong_type() -> &'static str {
    "a record holds a value of another type than its field"
}

/// Where a value goes: a field of a record, which writes the field's id
/// first and counts the field, or an element of a list or the key of a link.
enum Place<'a> {
    Field { id: u64, written: &'a mut u64 },
    Element,
}

/// Writes one value of a record: the value of a field, an element of a list,
/// or the key of a link.
pub struct ValueWriter<'a> {
    out: &'a mut Vec<u8>,
    place: Place<'a>,
    /// The layout of an embedded object, for a field that holds one.
    layout: Option<&'a Layout>,
}

impl<'a> ValueWriter<'a> {
    fn element(out: &'a mut Vec<u8>) -> Self {
        Self {
            out,
            place: Place::Element,
            layout: None,
        }
    }

    /// Writes the field's id, before its first byte, and counts it.
    fn begin(&mut self) {
        if let Place::Field { id, written } = &mut self.place {
            codec::write_varint(*id, self.out);
            **written += 1;
        }
    }

    /// No value: an optional field is left out of the record.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an element of a list or a link's key,
    /// which hold no null.
    pub fn null(self) -> Result<()> {
        match self.place {
            Place::Field { .. } => Ok(()),
            Place::Element => Err(Error::InvalidArgument {
                message: "a list holds no null".to_owned(),
            }),
        }
    }

    /// Writes a `bool`.
    ///
    /// # Errors
    ///
    /// None; the result keeps every method of the writer alike.
    pub fn bool(mut self, value: bool) -> Result<()> {
        self.begin();
        self.out.push(if value { TRUE } else { FALSE });

        Ok(())
    }

    /// Writes an `int`.
    ///
    /// # Errors
    ///
    /// None; see [`bool`](Self::bool).
    pub fn int(mut self, value: i64) -> Result<()> {
        self.begin();
        self.out.push(INT);
        codec::write_varint(codec::zigzag(value), self.out);

        Ok(())
    }

    /// Writes a `float`.
    ///
    /// # Errors
    ///
    /// None; see [`bool`](Self::bool).
    pub fn float(mut self, value: f64) -> Result<()> {
        self.begin();
        self.out.push(FLOAT);
        self.out.extend_from_slice(&value.to_le_bytes());

        Ok(())
    }

    /// Writes a `string`.
    ///
    /// # Errors
    ///
    /// None; see [`bool`](Self::bool).
    pub fn string(mut self, value: &str) -> Result<()> {
        self.begin();
        self.out.push(STRING);
        codec::write_varint(value.len() as u64, self.out);
        self.out.extend_from_slice(value.as_bytes());

        Ok(())
    }

    /// Writes `bytes`.
    ///
    /// # Errors
    ///
    /// None; see [`bool`](Self::bool).
    pub fn bytes(mut self, value: &[u8]) -> Result<()> {
        self.begin();
        self.out.push(BYTES);
        codec::write_varint(value.len() as u64, self.out);
        self.out.extend_from_slice(value);

        Ok(())
    }

    /// Writes a list of `values`.
    ///
    /// # Errors
    ///
    /// What writing an element returns.
    pub fn list<T: ElementType>(mut self, values: &[T]) -> Result<()> {
        self.begin();
        self.out.push(LIST);
        codec::write_varint(values.len() as u64, self.out);

        for value in values {
            value.write(ValueWriter::element(self.out))?;
        }

        Ok(())
    }

    /// Writes an embedded object.
    ///
    /// # Errors
    ///
    /// What writing a field of the object returns, and
    /// [`Error::InvalidArgument`] for a field whose type holds no embedded
    /// object.
    pub fn object<E: EmbeddedType>(mut self, object: &E) -> Result<()> {
        let layout = self.layout.ok_or_else(|| Error::InvalidArgument {
            message: format!(
                "`{}` is written to a field that holds no embedded object",
                std::any::type_name::<E>()
            ),
        })?;
        // An embedded record is preceded by its length, known once it is
        // written, so it is written apart first.
        let mut inner = Vec::new();

        encode(layout, &mut inner, |slot, value| {
            object.write_field(slot, value)
        })?;
        self.begin();
        self.out.push(OBJECT);
        codec::write_varint(inner.len() as u64, self.out);
        self.out.extend_from_slice(&inner);

        Ok(())
    }

    /// Writes a link to the object whose primary key is `key`.
    ///
    /// # Errors
    ///
    /// What writing the key returns.
    pub fn link<K: KeyType>(mut self, key: &K) -> Result<()> {
        self.begin();
        self.out.push(LINK);
        key.write(ValueWriter::element(self.out))
    }
}

/// Writes the record of an object or an embedded object whose fields lie as
/// `layout` says, each written by `write_field`, after the count of fields
/// written, into `out`.
fn encode(
    layout: &Layout,
    out: &mut Vec<u8>,
    mut write_field: impl FnMut(usize, ValueWriter<'_>) -> Result<()>,
) -> Result<()> {
    let start = out.len();
    let most = layout.fields.len() as u64;
    let (room, reserved) = codec::varint_bytes(most);

    out.extend_from_slice(&room[..reserved]);

    let mut written = 0;

    for field in &layout.fields {
        write_field(
            field.slot,
            ValueWriter {
                out: &mut *out,
                place: Place::Field {
                    id: field.id,
                    written: &mut written,
                },
                layout: field.nested.as_deref(),
            },
        )?;
    }

    if written != most {
        let (count, len) = codec::varint_bytes(written);

        out.splice(start..start + reserved, count[..len].iter().copied());
    }

    Ok(())
}

/// The error for damage found in a record, with what is known of where.
type Damaged<'a> = &'a dyn Fn(&str) -> Error;

/// Gives the fields of a record one at a time, each as the number its type
/// gives it and its value: the value the record holds, or the field's
/// default or null when the record leaves it out, as a record written before
/// the field existed does. Every field the type has comes once.
pub struct FieldReader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// The record's fields not read yet.
    left: usize,
    last: Option<u64>,
    /// A field of the record read, but not given yet: its id and where its
    /// value starts.
    pending: Option<(u64, usize)>,
    layout: &'a Layout,
    /// The next field of the layout.
    next: usize,
    damaged: Damaged<'a>,
}

impl fmt::Debug for FieldReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldReader")
            .field("at", &self.at)
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

impl<'a> FieldReader<'a> {
    fn new(bytes: &'a [u8], layout: &'a Layout, damaged: Damaged<'a>) -> Result<Self> {
        let (count, at) = codec::quick_varint(bytes, 0).map_err(damaged)?;
        // Two bytes a field at least: an id and a tag.
        let left = usize::try_from(count)
            .ok()
            .filter(|count| count.saturating_mul(2) <= bytes.len() - at)
            .ok_or_else(|| damaged("a record counts more than it holds"))?;

        Ok(Self {
            bytes,
            at,
            left,
            last: None,
            pending: None,
            layout,
            next: 0,
            damaged,
        })
    }

    /// The next field: its number in the type and its value, or `None` once
    /// every field has come.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a record that is damaged.
    #[expect(
        clippy::should_implement_trait,
        reason = "each value borrows the record the reader holds, which an iterator's items cannot"
    )]
    pub fn next(&mut self) -> Result<Option<(usize, ValueReader<'a>)>> {
        let damaged = self.damaged;

        loop {
            let Some(field) = self.layout.fields.get(self.next) else {
                self.finish()?;

                return Ok(None);
            };

            if self.pending.is_none() && self.left > 0 {
                let (id, at) = codec::quick_varint(self.bytes, self.at).map_err(damaged)?;

                if self.last.is_some_and(|last| last >= id) {
                    return Err(damaged("a record's field ids are out of order"));
                }

                self.last = Some(id);
                self.left -= 1;
                self.pending = Some((id, at));
            }

            match self.pending {
                Some((id, at)) if id < field.id => {
                    // A field the schema no longer has.
                    self.at = codec::skip_at(self.bytes, at).map_err(damaged)?;
                    self.pending = None;
                }
                Some((id, at)) if id == field.id => {
                    let end = codec::skip_scalar(self.bytes, at).map_err(damaged)?;

                    self.at = end;
                    self.pending = None;
                    self.next += 1;

                    return Ok(Some((
                        field.slot,
                        ValueReader {
                            bytes: &self.bytes[at..end],
                            layout: field.nested.as_deref(),
                            damaged,
                        },
                    )));
                }
                // The record leaves the field out.
                _ => {
                    self.next += 1;

                    let bytes = match &field.default {
                        Some(default) => default.as_slice(),
                        None if field.optional => &[],
                        None => return Err(damaged("a record lacks a required field")),
                    };

                    return Ok(Some((
                        field.slot,
                        ValueReader {
                            bytes,
                            layout: field.nested.as_deref(),
                            damaged,
                        },
                    )));
                }
            }
        }
    }

    /// Steps over the fields of the record after the last one the layout
    /// has, which belong to fields the schema no longer has, and checks that
    /// nothing follows them.
    fn finish(&mut self) -> Result<()> {
        let damaged = self.damaged;
        let mut at = match self.pending.take() {
            Some((_, at)) => codec::skip_at(self.bytes, at).map_err(damaged)?,
            None => self.at,
        };

        while self.left > 0 {
            let (id, value) = codec::quick_varint(self.bytes, at).map_err(damaged)?;

            if self.last.is_some_and(|last| last >= id) {
                return Err(damaged("a record's field ids are out of order"));
            }

            self.last = Some(id);
            self.left -= 1;
            at = codec::skip_at(self.bytes, value).map_err(damaged)?;
        }

        self.at = at;

        if at != self.bytes.len() {
            return Err(damaged("a record has bytes after its last field"));
        }

        Ok(())
    }

    /// The value a field's slot was given, for the code that reads every
    /// field into a variable of its own and builds the object at the end.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] if the field never came, which a reader that
    /// gives every field once does not allow; it means a hand-written
    /// [`CollectionType`] numbered a field the layout does not.
    pub fn take<T>(&self, value: Option<T>) -> Result<T> {
        value.ok_or_else(|| (self.damaged)("a field of the type never came"))
    }
}

/// One value of a record, borrowed from it: a field's, an element of a list,
/// or a link's key. A field the record leaves out reads as its default, and
/// an optional one without a default as null.
pub struct ValueReader<'a> {
    /// The value's tag and what follows it; empty for null.
    bytes: &'a [u8],
    layout: Option<&'a Layout>,
    damaged: Damaged<'a>,
}

impl fmt::Debug for ValueReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueReader")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl<'a> ValueReader<'a> {
    /// Whether the value is null.
    pub fn is_null(&self) -> bool {
        self.bytes.is_empty()
    }

    fn fail(&self, reason: &str) -> Error {
        (self.damaged)(reason)
    }

    /// The tag, and the bytes after it.
    fn tagged(&self, tag: u8) -> Result<&'a [u8]> {
        match self.bytes.split_first() {
            Some((&found, rest)) if found == tag => Ok(rest),
            Some(_) => Err(self.fail(wrong_type())),
            None => Err(self.fail("a record holds null for a required field")),
        }
    }

    /// A length and the bytes it counts, after a tag, which end the value.
    fn counted(&self, tag: u8) -> Result<&'a [u8]> {
        let rest = self.tagged(tag)?;
        let (len, start) = codec::quick_varint(rest, 0).map_err(|reason| self.fail(reason))?;
        let end = codec::skip_bytes(rest, start, len).map_err(|reason| self.fail(reason))?;

        Ok(&rest[start..end])
    }

    /// The value as a `bool`.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type.
    pub fn bool(self) -> Result<bool> {
        match self.bytes.first() {
            Some(&TRUE) => Ok(true),
            Some(&FALSE) => Ok(false),
            Some(_) => Err(self.fail(wrong_type())),
            None => Err(self.fail("a record holds null for a required field")),
        }
    }

    /// The value as an `int`.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type.
    pub fn int(self) -> Result<i64> {
        let rest = self.tagged(INT)?;
        let (value, _) = codec::quick_varint(rest, 0).map_err(|reason| self.fail(reason))?;

        Ok(codec::unzigzag(value))
    }

    /// The value as a `float`.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type.
    pub fn float(self) -> Result<f64> {
        let rest = self.tagged(FLOAT)?;
        let exact = rest
            .first_chunk::<8>()
            .ok_or_else(|| self.fail("a record ends early"))?;

        Ok(f64::from_le_bytes(*exact))
    }

    /// The value as a `string`, borrowed from the record.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type, or text that is not
    /// UTF-8.
    pub fn str(self) -> Result<&'a str> {
        let bytes = self.counted(STRING)?;

        std::str::from_utf8(bytes)
            .map_err(|_| self.fail("a record holds a string that is not UTF-8"))
    }

    /// The value as `bytes`, borrowed from the record.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type.
    pub fn bytes(self) -> Result<&'a [u8]> {
        self.counted(BYTES)
    }

    /// The value as a list.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type, or an element that
    /// does not read as `T`.
    pub fn list<T: ElementType>(self) -> Result<Vec<T>> {
        let rest = self.tagged(LIST)?;
        let (count, mut at) = codec::quick_varint(rest, 0).map_err(|reason| self.fail(reason))?;
        // A byte an element at least.
        let count = usize::try_from(count)
            .ok()
            .filter(|&count| count <= rest.len() - at)
            .ok_or_else(|| self.fail("a record counts more than it holds"))?;
        let mut values = Vec::with_capacity(count);

        for _ in 0..count {
            let end = codec::skip_scalar(rest, at).map_err(|reason| self.fail(reason))?;

            values.push(T::read(ValueReader {
                bytes: &rest[at..end],
                layout: None,
                damaged: self.damaged,
            })?);
            at = end;
        }

        Ok(values)
    }

    /// The value as an embedded object.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type, or a record that
    /// does not read as `E`.
    pub fn object<E: EmbeddedType>(self) -> Result<E> {
        let layout = self
            .layout
            .ok_or_else(|| self.fail("an embedded object is read from a field that holds none"))?;
        let record = self.counted(OBJECT)?;

        E::read(FieldReader::new(record, layout, self.damaged)?)
    }

    /// The value as a link: the primary key of the object linked to.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupted`] for a value of another type.
    pub fn link<K: KeyType>(self) -> Result<K> {
        let rest = self.tagged(LINK)?;

        K::read(ValueReader {
            bytes: rest,
            layout: None,
            damaged: self.damaged,
        })
    }
}

impl FieldType for bool {
    fn kind() -> Type {
        Type::Bool
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.bool(*self)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.bool()
    }
}

impl FieldType for i64 {
    fn kind() -> Type {
        Type::Int
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.int(*self)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.int()
    }
}

impl FieldType for f64 {
    fn kind() -> Type {
        Type::Float
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.float(*self)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.float()
    }
}

impl FieldType for String {
    fn kind() -> Type {
        Type::String
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.string(self)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.str().map(str::to_owned)
    }
}

/// `bytes`. A list of small ints would be `Vec<i64>`.
impl FieldType for Vec<u8> {
    fn kind() -> Type {
        Type::Bytes
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.bytes(self)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.bytes().map(<[u8]>::to_vec)
    }
}

impl<T: FieldType> FieldType for Option<T> {
    const OPTIONAL: bool = true;

    fn kind() -> Type {
        T::kind()
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        match self {
            Some(inner) => inner.write(value),
            None => value.null(),
        }
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        if value.is_null() {
            Ok(None)
        } else {
            T::read(value).map(Some)
        }
    }
}

impl<T: ElementType> FieldType for Vec<T> {
    fn kind() -> Type {
        Type::list(T::kind())
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.list(self)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.list()
    }
}

impl<T: CollectionType> FieldType for Link<T> {
    fn kind() -> Type {
        Type::link(T::COLLECTION)
    }

    fn write(&self, value: ValueWriter<'_>) -> Result<()> {
        value.link(&self.key)
    }

    fn read(value: ValueReader<'_>) -> Result<Self> {
        value.link().map(Link::new)
    }
}

impl ElementType for bool {}
impl ElementType for i64 {}
impl ElementType for f64 {}
impl ElementType for String {}
impl ElementType for Vec<u8> {}
impl<T: CollectionType> ElementType for Link<T> {}

impl KeyType for i64 {
    fn to_value(&self) -> Value {
        Value::Int(*self)
    }

    fn from_value(value: Value) -> Result<Self> {
        value.as_int().ok_or_else(|| key_of_another_type(&value))
    }
}

impl KeyType for String {
    fn to_value(&self) -> Value {
        Value::String(self.clone())
    }

    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::String(key) => Ok(key),
            value => Err(key_of_another_type(&value)),
        }
    }
}

impl KeyType for Vec<u8> {
    fn to_value(&self) -> Value {
        Value::Bytes(self.clone())
    }

    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Bytes(key) => Ok(key),
            value => Err(key_of_another_type(&value)),
        }
    }
}

fn key_of_another_type(value: &Value) -> Error {
    Error::InvalidArgument {
        message: format!("a primary key of another type than the field's: {value:?}"),
    }
}

/// How the fields of a type lie in a stored collection or embedded object.
#[derive(Debug)]
pub(crate) struct Layout {
    /// The stored fields, in id order.
    fields: Vec<Slot>,
}

/// A stored field, and what reading and writing it needs.
#[derive(Debug)]
struct Slot {
    id: u64,
    /// The field's number in the type.
    slot: usize,
    optional: bool,
    /// The field's default as a record holds it, tag first.
    default: Option<Vec<u8>>,
    nested: Option<Box<Layout>>,
}

/// How a type lies in a collection: the collection's position in the stored
/// schema, and the layout of its fields.
#[derive(Debug)]
pub(crate) struct Typed {
    position: usize,
    layout: Layout,
}

/// The layout of the fields `declared` in the stored `fields` of `owner`, or
/// what keeps them apart: a field one has and the other lacks, or a field of
/// another type in each.
fn layout_of(
    declared: &[Field],
    stored: &Fields,
    schema: &StoredSchema,
    owner: &str,
) -> std::result::Result<Layout, String> {
    if let Some(field) = declared
        .iter()
        .find(|field| stored.by_name(&field.name).is_none())
    {
        return Err(format!(
            "the type has a field `{}` that `{owner}` does not",
            field.name
        ));
    }

    let mut fields = Vec::with_capacity(stored.list.len());

    for field in &stored.list {
        let slot = declared
            .iter()
            .position(|declared| declared.name == field.name)
            .ok_or_else(|| {
                format!(
                    "`{owner}` has a field `{}` that the type does not",
                    field.name
                )
            })?;
        let declared = &declared[slot];
        let path = format!("{owner}.{}", field.name);

        if declared.optional != field.optional {
            return Err(format!(
                "`{path}` is optional in one of them and required in the other"
            ));
        }

        let nested = match (&declared.kind, &field.kind) {
            (Type::Object(embedded), Kind::Object(inner)) => {
                Some(Box::new(layout_of(&embedded.fields, inner, schema, &path)?))
            }
            (kind, stored) if same_kind(kind, stored, schema) => None,
            _ => return Err(format!("`{path}` has another type in each")),
        };
        let default = field
            .default
            .as_ref()
            .map(|value| {
                let mut bytes = Vec::new();

                codec::encode_value(value, &field.kind, &|id| schema.key_kind(id), &mut bytes)
                    .map(|()| bytes)
            })
            .transpose()
            .map_err(|expected| format!("the default of `{path}` holds {expected}"))?;

        fields.push(Slot {
            id: field.id,
            slot,
            optional: field.optional,
            default,
            nested,
        });
    }

    Ok(Layout { fields })
}

/// Whether a declared type and a stored kind hold the same values, an
/// embedded object aside, which [`layout_of`] matches field by field.
fn same_kind(declared: &Type, stored: &Kind, schema: &StoredSchema) -> bool {
    match (declared, stored) {
        (Type::Bool, Kind::Bool)
        | (Type::Int, Kind::Int)
        | (Type::Float, Kind::Float)
        | (Type::String, Kind::String)
        | (Type::Bytes, Kind::Bytes) => true,
        (Type::Link(target), Kind::Link { collection }) => schema
            .collection_by_id(*collection)
            .is_some_and(|stored| &stored.name == target),
        (Type::List(element), Kind::List(inner)) => same_kind(element, inner, schema),
        _ => false,
    }
}

/// How `T` lies in the collection of the same name in `schema`.
fn typed_layout<T: CollectionType>(schema: &StoredSchema) -> Result<Typed> {
    let position = position(schema, T::COLLECTION)?;
    let declared = T::collection();

    if declared.name != T::COLLECTION {
        return Err(Error::InvalidArgument {
            message: format!(
                "`{}` declares the collection `{}` but names it `{}`",
                std::any::type_name::<T>(),
                declared.name,
                T::COLLECTION
            ),
        });
    }

    let stored = &schema.collections[position];
    let refuse = |reason: &str| Error::InvalidArgument {
        message: format!(
            "`{}` does not match the collection `{}` the database holds: {reason}",
            std::any::type_name::<T>(),
            declared.name
        ),
    };
    let same_key = match &declared.key {
        None => stored.auto,
        Some(name) => !stored.auto && stored.key_field().is_some_and(|key| &key.name == name),
    };

    if !same_key {
        return Err(refuse("their primary keys differ"));
    }

    let layout = layout_of(
        &declared.all_fields(),
        &stored.fields,
        schema,
        &declared.name,
    )
    .map_err(|reason| refuse(&reason))?;

    Ok(Typed { position, layout })
}

/// How `T` lies in the collection of `schema`, worked out the first time a
/// transaction of the handle asks and kept with the schema after that.
fn typed_of<T: CollectionType>(schema: &OpenSchema) -> Result<Arc<Typed>> {
    let id = TypeId::of::<T>();

    if let Some(Ok(typed)) = schema.typed(id).map(Arc::downcast::<Typed>) {
        return Ok(typed);
    }

    let typed = Arc::new(typed_layout::<T>(&schema.schema)?);

    schema.keep_typed(id, Arc::clone(&typed) as Arc<dyn Any + Send + Sync>);

    Ok(typed)
}

/// The error for damage found in an object of `collection` that `source`
/// read.
fn damaged_in<'s>(source: &'s dyn Source, collection: &'s str) -> impl Fn(&str) -> Error + 's {
    move |reason| source.corrupted(format!("an object of `{collection}`: {reason}"))
}

/// The object of `T` whose record is `bytes`.
fn read_record<T: CollectionType>(
    bytes: &[u8],
    layout: &Layout,
    damaged: Damaged<'_>,
) -> Result<T> {
    T::read(FieldReader::new(bytes, layout, damaged)?)
}

/// A collection of a read transaction, as `T`: its objects as of the
/// transaction's commit, read straight into `T`.
pub struct TypedReader<'a, T> {
    inner: CollectionReader<'a>,
    typed: Arc<Typed>,
    target: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for TypedReader<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedReader")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<'a, T: CollectionType> TypedReader<'a, T> {
    pub(crate) fn new(txn: &'a ReadTransaction) -> Result<Self> {
        let schema = checked_schema(txn, txn.schema(), txn.schema_checked())?;
        let typed = typed_of::<T>(&schema)?;

        Ok(Self {
            inner: CollectionReader::at(txn, schema, typed.position),
            typed,
            target: PhantomData,
        })
    }

    /// The collection, as [`ReadTransaction::collection`] gives it, for
    /// what the typed reader does not offer.
    pub fn untyped(&self) -> &CollectionReader<'a> {
        &self.inner
    }

    /// The object of `T` whose record is `bytes`.
    pub(crate) fn read(&self, bytes: &[u8]) -> Result<T> {
        let (source, _, collection) = self.inner.parts();

        read_record(
            bytes,
            &self.typed.layout,
            &damaged_in(source, &collection.name),
        )
    }

    /// The object whose primary key is `key`, if there is one.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key of another type than the
    /// collection's, and [`Error::Corrupted`] for a record that does not
    /// read as `T`.
    pub fn get(&self, key: impl Into<Value>) -> Result<Option<T>> {
        let mut found = None;

        self.inner.get_record_with(key, |bytes| {
            found = Some(self.read(bytes)?);

            Ok(())
        })?;

        Ok(found)
    }

    /// Every object, in primary key order.
    ///
    /// # Errors
    ///
    /// What reading the file returns; each object is `Err` if its record
    /// does not read as `T`.
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_> {
        let (source, _, collection) = self.inner.parts();
        let range = source.range_in(
            &records(collection.id),
            Bound::Unbounded,
            Bound::Unbounded,
            false,
        )?;

        Ok(range.map(move |entry| entry.and_then(|(_, bytes)| self.read(&bytes))))
    }

    /// The number of objects.
    ///
    /// # Errors
    ///
    /// What reading the file returns.
    pub fn len(&self) -> Result<u64> {
        self.inner.len()
    }

    /// Whether the collection holds no object.
    ///
    /// # Errors
    ///
    /// What reading the file returns.
    pub fn is_empty(&self) -> Result<bool> {
        self.inner.is_empty()
    }
}

/// A collection of a write transaction, as `T`: its objects read straight
/// into `T`, and `T`'s values written as its objects.
pub struct TypedWriter<'a, T> {
    inner: CollectionWriter<'a>,
    typed: Arc<Typed>,
    /// The record of the object being written, kept for the next write.
    record: Vec<u8>,
    target: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for TypedWriter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedWriter")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<'a, T: CollectionType> TypedWriter<'a, T> {
    pub(crate) fn new(txn: &'a mut WriteTransaction) -> Result<Self> {
        let schema = checked_schema(txn, txn.schema(), txn.schema_checked())?;
        let typed = typed_of::<T>(&schema)?;

        Ok(Self {
            inner: CollectionWriter::at(txn, schema, typed.position),
            typed,
            record: Vec::new(),
            target: PhantomData,
        })
    }

    /// The collection, as [`WriteTransaction::collection`] gives it, for
    /// what the typed writer does not offer.
    pub fn untyped(&mut self) -> &mut CollectionWriter<'a> {
        &mut self.inner
    }

    /// The collection, borrowed for reading.
    pub(crate) fn untyped_ref(&self) -> &CollectionWriter<'a> {
        &self.inner
    }

    /// The object of `T` whose record is `bytes`.
    pub(crate) fn read(&self, bytes: &[u8]) -> Result<T> {
        let (source, _, collection) = self.inner.parts();

        read_record(
            bytes,
            &self.typed.layout,
            &damaged_in(source, &collection.name),
        )
    }

    fn encode(&mut self, object: &T) -> Result<()> {
        self.record.clear();
        encode(&self.typed.layout, &mut self.record, |slot, value| {
            object.write_field(slot, value)
        })
    }

    /// Inserts `object` and returns its primary key. In a collection keyed
    /// by an auto-increment, an object whose `id` is `None` gets the next
    /// number.
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::insert`] fails, leaving the transaction as it
    /// was.
    pub fn insert(&mut self, object: &T) -> Result<T::Key> {
        self.encode(object)?;

        let key = self.inner.insert_record(&self.record)?;

        T::Key::from_value(key)
    }

    /// Inserts `object`, or replaces the object with its primary key, and
    /// returns the key.
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::put`] fails, leaving the transaction as it was.
    pub fn put(&mut self, object: &T) -> Result<T::Key> {
        self.encode(object)?;

        let key = self.inner.put_record(&self.record)?;

        T::Key::from_value(key)
    }

    /// Deletes the object whose primary key is `key`, and returns whether
    /// there was one.
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::delete`] fails.
    pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool> {
        self.inner.delete(key)
    }

    /// Sets the fields `changes` has in the object whose primary key is
    /// `key`, as [`CollectionWriter::update`] does, and returns whether there
    /// was one.
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::update`] fails.
    pub fn update(&mut self, key: impl Into<Value>, changes: crate::Object) -> Result<bool> {
        self.inner.update(key, changes)
    }

    /// The object whose primary key is `key`, if there is one, with this
    /// transaction's changes.
    ///
    /// # Errors
    ///
    /// As [`TypedReader::get`] fails.
    pub fn get(&self, key: impl Into<Value>) -> Result<Option<T>> {
        let mut found = None;

        self.inner.get_record_with(key, |bytes| {
            found = Some(self.read(bytes)?);

            Ok(())
        })?;

        Ok(found)
    }

    /// Every object, in primary key order, with this transaction's changes.
    ///
    /// # Errors
    ///
    /// As [`TypedReader::iter`] fails.
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_> {
        let (source, _, collection) = self.inner.parts();
        let range = source.range_in(
            &records(collection.id),
            Bound::Unbounded,
            Bound::Unbounded,
            false,
        )?;

        Ok(range.map(move |entry| entry.and_then(|(_, bytes)| self.read(&bytes))))
    }

    /// The number of objects, with this transaction's changes.
    ///
    /// # Errors
    ///
    /// What reading the file returns.
    pub fn len(&self) -> Result<u64> {
        self.inner.len()
    }

    /// Whether the collection holds no object.
    ///
    /// # Errors
    ///
    /// What reading the file returns.
    pub fn is_empty(&self) -> Result<bool> {
        self.inner.is_empty()
    }
}

/// What the code `#[derive(Object)]` and `#[derive(Embedded)]` generate
/// calls, and nothing else should: it may change in any release.
#[doc(hidden)]
pub mod derive {
    use super::{Collection, Embedded, FieldType};
    use crate::format::object::Value;

    /// `collection` with field `name` of type `T` added, optional if `T` is,
    /// with `default` if it has one.
    pub fn field<T: FieldType>(
        collection: Collection,
        name: &str,
        default: Option<Value>,
    ) -> Collection {
        match default {
            Some(default) => collection.with_default(name, T::kind(), default),
            None if T::OPTIONAL => collection.optional(name, T::kind()),
            None => collection.field(name, T::kind()),
        }
    }

    /// `collection` with field `name` of type `T` added as its primary key.
    pub fn key<T: FieldType>(collection: Collection, name: &str) -> Collection {
        collection.primary_key(name, T::kind())
    }

    /// `embedded` with field `name` of type `T` added, as [`field`] adds one
    /// to a collection.
    pub fn embedded_field<T: FieldType>(
        embedded: Embedded,
        name: &str,
        default: Option<Value>,
    ) -> Embedded {
        match default {
            Some(default) => embedded.with_default(name, T::kind(), default),
            None if T::OPTIONAL => embedded.optional(name, T::kind()),
            None => embedded.field(name, T::kind()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::format::object::codec::Raw;
    use crate::schema::Schema;
    use crate::schema::resolve::{Resolution, resolve};

    /// A type written by hand, as a derive would write it: the
    /// auto-increment `id` in slot 0, then `name` and `note`.
    #[derive(Debug, Clone, PartialEq)]
    struct Note {
        id: Option<i64>,
        name: String,
        note: Option<String>,
    }

    impl CollectionType for Note {
        type Key = i64;

        const COLLECTION: &'static str = "notes";

        fn collection() -> Collection {
            Collection::new("notes")
                .field("name", Type::String)
                .optional("note", Type::String)
        }

        fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> Result<()> {
            match slot {
                0 => self.id.write(value),
                1 => self.name.write(value),
                2 => self.note.write(value),
                _ => Ok(()),
            }
        }

        fn read(mut fields: FieldReader<'_>) -> Result<Self> {
            let (mut id, mut name, mut note) = (None, None, None);

            while let Some((slot, value)) = fields.next()? {
                match slot {
                    0 => id = Some(FieldType::read(value)?),
                    1 => name = Some(FieldType::read(value)?),
                    2 => note = Some(FieldType::read(value)?),
                    _ => {}
                }
            }

            Ok(Self {
                id: fields.take(id)?,
                name: fields.take(name)?,
                note: fields.take(note)?,
            })
        }
    }

    /// The layout of `Note` in a file whose first schema declared it: `id`
    /// is field 1, `name` 2 and `note` 3.
    fn layout() -> Layout {
        let schema = Schema::new(1).collection(Note::collection());
        let stored = match resolve(&schema, None, &[]) {
            Ok(Resolution::Change(plan)) => plan.to,
            _ => panic!("a first schema is stored"),
        };

        match typed_layout::<Note>(&stored) {
            Ok(typed) => typed.layout,
            Err(error) => panic!("{error}"),
        }
    }

    fn read(bytes: &[u8]) -> Result<Note> {
        let damaged = |reason: &str| Error::Corrupted {
            path: PathBuf::new(),
            reason: reason.to_owned(),
        };

        read_record(bytes, &layout(), &damaged)
    }

    fn reason(bytes: &[u8]) -> String {
        match read(bytes) {
            Err(Error::Corrupted { reason, .. }) => reason,
            other => panic!("{other:?}"),
        }
    }

    fn written(note: &Note) -> Vec<u8> {
        let mut out = Vec::new();

        encode(&layout(), &mut out, |slot, value| {
            note.write_field(slot, value)
        })
        .unwrap_or_else(|error| panic!("{error}"));
        out
    }

    #[test]
    fn a_record_written_from_a_type_reads_back_as_it() {
        let notes = [
            Note {
                id: Some(7),
                name: "a".to_owned(),
                note: Some("é\0z".to_owned()),
            },
            Note {
                id: None,
                name: String::new(),
                note: None,
            },
        ];

        for note in notes {
            let bytes = written(&note);

            // The same bytes the record format writes for those values.
            let mut fields = Vec::new();

            if let Some(id) = note.id {
                fields.push((1, Raw::Int(id)));
            }

            fields.push((2, Raw::String(note.name.clone())));

            if let Some(text) = &note.note {
                fields.push((3, Raw::String(text.clone())));
            }

            assert_eq!(bytes, codec::write(&fields));

            if note.id.is_some() {
                assert_eq!(read(&bytes).ok(), Some(note));
            }
        }
    }

    #[test]
    fn fields_a_record_leaves_out_read_as_null_and_removed_ones_are_skipped() {
        let record = codec::write(&[
            (1, Raw::Int(1)),
            (2, Raw::String("x".to_owned())),
            (5, Raw::List(vec![Raw::Int(1), Raw::Int(2)])),
            (9, Raw::Object(vec![(1, Raw::Bool(true))])),
        ]);

        assert_eq!(
            read(&record).ok(),
            Some(Note {
                id: Some(1),
                name: "x".to_owned(),
                note: None,
            })
        );

        // An id below every field the schema has.
        let record = codec::write(&[
            (0, Raw::String("removed".to_owned())),
            (1, Raw::Int(1)),
            (2, Raw::String("x".to_owned())),
            (3, Raw::String("kept".to_owned())),
        ]);

        assert_eq!(
            read(&record).ok().and_then(|note| note.note),
            Some("kept".to_owned())
        );
    }

    #[test]
    fn damaged_records_are_corrupted() {
        let name = || (2, Raw::String("x".to_owned()));

        assert_eq!(
            reason(&codec::write(&[(1, Raw::Int(1))])),
            "a record lacks a required field"
        );
        assert_eq!(
            reason(&codec::write(&[(1, Raw::Int(1)), (2, Raw::Int(3))])),
            wrong_type()
        );

        let mut out_of_order = codec::write(&[(1, Raw::Int(1)), name()]);

        // `name` twice: a count of 3, and the field again.
        out_of_order[0] = 3;
        out_of_order.extend_from_slice(&codec::write(&[name()])[1..]);
        assert_eq!(
            reason(&out_of_order),
            "a record's field ids are out of order"
        );

        let mut trailing = codec::write(&[(1, Raw::Int(1)), name()]);

        trailing.push(0);
        assert_eq!(reason(&trailing), "a record has bytes after its last field");

        let whole = codec::write(&[(1, Raw::Int(1)), name()]);

        assert_eq!(reason(&whole[..whole.len() - 1]), "a record ends early");
        assert_eq!(reason(&[9, 1, 4]), "a record counts more than it holds");

        let mut not_utf8 = codec::write(&[(1, Raw::Int(1)), (2, Raw::String("é".to_owned()))]);
        let last = not_utf8.len() - 1;

        not_utf8[last] = 0xFF;
        assert_eq!(
            reason(&not_utf8),
            "a record holds a string that is not UTF-8"
        );
    }

    #[test]
    fn random_bytes_read_as_an_object_or_as_damage_and_never_panic() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let base = codec::write(&[
            (1, Raw::Int(1)),
            (2, Raw::String("name".to_owned())),
            (3, Raw::String("note".to_owned())),
        ]);

        for _ in 0..20_000 {
            let mut bytes = base.clone();
            let changes = next() % 4 + 1;

            for _ in 0..changes {
                let at = usize::try_from(next() % bytes.len() as u64).unwrap_or(0);
                let byte = next().to_le_bytes()[0];

                match next() % 3 {
                    0 => bytes[at] = byte,
                    1 => bytes.insert(at, byte),
                    _ => {
                        bytes.truncate(at.max(1));
                    }
                }
            }

            match read(&bytes) {
                Ok(_) | Err(Error::Corrupted { .. }) => {}
                Err(other) => panic!("{other:?}"),
            }
        }
    }
}
