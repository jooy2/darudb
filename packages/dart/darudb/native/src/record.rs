//! Reading the few records the Dart side sends that the engine does not read
//! itself: the options of `Database.open`, its migrations, and primary keys.
//! They use the record format of `design/objects.md`, "Records", which the
//! Dart side writes anyway, so no second format exists for them.
//!
//! What comes from Dart is the package's own code, but it is still checked:
//! a length past the end or a tag where another was expected is
//! `INVALID_ARGUMENT`, never a panic.

use crate::{Failure, Result, invalid};

pub(crate) const FALSE: u8 = 0x02;
pub(crate) const TRUE: u8 = 0x03;
pub(crate) const INT: u8 = 0x04;
pub(crate) const STRING: u8 = 0x06;
pub(crate) const BYTES: u8 = 0x07;
pub(crate) const LIST: u8 = 0x08;
pub(crate) const OBJECT: u8 = 0x09;

pub(crate) fn zigzag(value: i64) -> u64 {
    u64::from_le_bytes(((value << 1) ^ (value >> 63)).to_le_bytes())
}

fn unzigzag(value: u64) -> i64 {
    i64::from_le_bytes((value >> 1).to_le_bytes()) ^ -i64::from_le_bytes((value & 1).to_le_bytes())
}

fn ends_early() -> Failure {
    invalid("a record from the Dart side ends early")
}

/// A position in a record.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn byte(&mut self) -> Result<u8> {
        let byte = *self.bytes.get(self.at).ok_or_else(ends_early)?;

        self.at += 1;

        Ok(byte)
    }

    fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;

        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;

            value |= u64::from(byte & 0x7F) << shift;

            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }

        Err(invalid(
            "a record from the Dart side holds a number too long",
        ))
    }

    fn take(&mut self, len: u64) -> Result<&'a [u8]> {
        let len = usize::try_from(len).map_err(|_| ends_early())?;
        let end = self
            .at
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(ends_early)?;
        let bytes = &self.bytes[self.at..end];

        self.at = end;

        Ok(bytes)
    }

    /// The bytes after a length, as a batch of records holds each record.
    pub(crate) fn counted(&mut self) -> Result<&'a [u8]> {
        let len = self.varint()?;

        self.take(len)
    }

    /// The fields of a record: its count, then each field's id and value.
    pub(crate) fn fields(mut self) -> Result<impl Iterator<Item = Result<(u64, Field<'a>)>>> {
        let count = self.varint()?;
        let mut reader = self;

        Ok((0..count).map(move |_| {
            let id = reader.varint()?;

            Ok((id, reader.value()?))
        }))
    }

    /// The value next: its tag and the bytes it takes.
    pub(crate) fn value(&mut self) -> Result<Field<'a>> {
        let tag = self.byte()?;
        let start = self.at;

        match tag {
            FALSE | TRUE => {}
            INT => {
                self.varint()?;
            }
            STRING | BYTES | OBJECT => {
                self.counted()?;
            }
            LIST => {
                let count = self.varint()?;

                for _ in 0..count {
                    self.value()?;
                }
            }
            _ => {
                return Err(invalid(
                    "a record from the Dart side holds a tag the library does not read",
                ));
            }
        }

        Ok(Field {
            tag,
            bytes: &self.bytes[start..self.at],
        })
    }

    /// The strings in fields 1 to `N` of a record, in that order.
    pub(crate) fn strings<const N: usize>(self) -> Result<[String; N]> {
        let mut found: [Option<String>; N] = std::array::from_fn(|_| None);

        for field in self.fields()? {
            let (id, value) = field?;
            let slot = usize::try_from(id)
                .ok()
                .and_then(|id| id.checked_sub(1))
                .and_then(|at| found.get_mut(at))
                .ok_or_else(|| {
                    invalid(format!(
                        "a record from the Dart side has a field {id} it should not"
                    ))
                })?;

            *slot = Some(value.string()?);
        }

        let mut strings = found.into_iter();

        Ok(std::array::from_fn(|_| {
            strings.next().flatten().unwrap_or_default()
        }))
    }
}

/// A value of a record: its tag, and the bytes after it.
pub(crate) struct Field<'a> {
    tag: u8,
    bytes: &'a [u8],
}

impl<'a> Field<'a> {
    fn expect(&self, tag: u8, what: &str) -> Result<Reader<'a>> {
        if self.tag == tag {
            Ok(Reader::new(self.bytes))
        } else {
            Err(invalid(format!(
                "a record from the Dart side holds something else where {what} belongs"
            )))
        }
    }

    pub(crate) fn bool(&self) -> Result<bool> {
        match self.tag {
            TRUE => Ok(true),
            FALSE => Ok(false),
            _ => Err(invalid(
                "a record from the Dart side holds something else where a bool belongs",
            )),
        }
    }

    pub(crate) fn int(&self) -> Result<i64> {
        self.expect(INT, "an int")?.varint().map(unzigzag)
    }

    pub(crate) fn bytes(&self) -> Result<&'a [u8]> {
        self.expect(BYTES, "bytes")?.counted()
    }

    pub(crate) fn string(&self) -> Result<String> {
        let bytes = self.expect(STRING, "a string")?.counted()?;

        String::from_utf8(bytes.to_vec())
            .map_err(|_| invalid("a string from the Dart side that is not UTF-8"))
    }

    pub(crate) fn object(&self) -> Result<Reader<'a>> {
        let mut reader = self.expect(OBJECT, "an object")?;

        reader.counted().map(Reader::new)
    }

    pub(crate) fn list(&self) -> Result<impl Iterator<Item = Result<Field<'a>>>> {
        let mut reader = self.expect(LIST, "a list")?;
        let count = reader.varint()?;

        Ok((0..count).map(move |_| reader.value()))
    }

    /// The value as a primary key: an `int`, a `string` or `bytes`.
    pub(crate) fn key(&self) -> Result<darudb::Value> {
        match self.tag {
            INT => self.int().map(darudb::Value::Int),
            STRING => self.string().map(darudb::Value::String),
            BYTES => self
                .bytes()
                .map(|bytes| darudb::Value::Bytes(bytes.to_vec())),
            _ => Err(invalid("a primary key is an int, a string or bytes")),
        }
    }
}
