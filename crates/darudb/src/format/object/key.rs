//! Values as keys: bytes that order as the values do.
//!
//! The kernel orders keys as unsigned bytes and nothing else, so a primary key
//! or an indexed value is encoded to keep its order in its bytes
//! (`design/objects.md`, "Keys"). Each encoding is a tag and the value, and
//! ends where it can be told to end, so encodings concatenate: the encoding of
//! a value followed by a primary key sorts by the value, then the key.

use std::cmp::Ordering;

use super::codec::FieldRef;
use super::value::Value;

pub(crate) const NULL: u8 = 0x01;
const FALSE: u8 = 0x02;
const TRUE: u8 = 0x03;
/// The tag of zero. An int is written in the fewest big-endian bytes that
/// hold it, and its tag says how many: `ZERO + n` for a positive int in `n`
/// bytes and `ZERO - n` for a negative one, from `INT` for the longest
/// negative ints to `INT_LAST` for the longest positive ones.
const ZERO: u8 = 0x0C;
const INT: u8 = ZERO - 8;
const INT_LAST: u8 = ZERO + 8;
const FLOAT: u8 = INT_LAST + 1;
const STRING: u8 = INT_LAST + 2;
const BYTES: u8 = INT_LAST + 3;

/// The first tag of each scalar type, in the order the types sort, and the
/// tag after the last: a type's encodings lie from its tag up to, but not
/// including, the next type's.
pub(crate) const BOOL_TAGS: (u8, u8) = (FALSE, INT);
pub(crate) const INT_TAGS: (u8, u8) = (INT, FLOAT);
pub(crate) const FLOAT_TAGS: (u8, u8) = (FLOAT, STRING);
pub(crate) const STRING_TAGS: (u8, u8) = (STRING, BYTES);
pub(crate) const BYTES_TAGS: (u8, u8) = (BYTES, BYTES + 1);

/// The bit that flips the sign of a 64-bit integer's bits.
const SIGN: u64 = 1 << 63;

/// The one NaN the engine stores.
const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

/// Appends the key encoding of `value` to `out`. Only a scalar value has one:
/// a list or an embedded object is refused.
pub(crate) fn encode(value: &Value, out: &mut Vec<u8>) -> Result<(), &'static str> {
    match value {
        Value::Null => out.push(NULL),
        Value::Bool(false) => out.push(FALSE),
        Value::Bool(true) => out.push(TRUE),
        Value::Int(value) => int(*value, out),
        Value::Float(value) => number(FLOAT, ordered_float(*value), out),
        Value::String(value) => escape(STRING, value.as_bytes(), out),
        Value::Bytes(value) => escape(BYTES, value, out),
        Value::List(_) | Value::Object(_) => return Err("a list or an object is not a key"),
    }

    Ok(())
}

/// [`encode`] for a scalar where a record holds it: the encoding of the
/// value the record reads as, with no value made for it. An encoding of a
/// list, an object or a link is refused, as a list or an object is.
pub(crate) fn encode_field(value: FieldRef<'_>, out: &mut Vec<u8>) -> Result<(), &'static str> {
    match value {
        FieldRef::Bool(false) => out.push(FALSE),
        FieldRef::Bool(true) => out.push(TRUE),
        FieldRef::Int(value) => int(value, out),
        FieldRef::Float(value) => number(FLOAT, ordered_float(value), out),
        FieldRef::String(value) => escape(STRING, value, out),
        FieldRef::Bytes(value) => escape(BYTES, value, out),
        FieldRef::Encoded(_) => return Err("only a scalar is encoded where a record holds it"),
    }

    Ok(())
}

/// The key encoding of `value` on its own.
pub(crate) fn encoded(value: &Value) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();

    encode(value, &mut out)?;

    Ok(out)
}

/// Reads the key encoding at the start of `bytes`. Returns the value and how
/// many bytes it took.
pub(crate) fn decode(bytes: &[u8]) -> Result<(Value, usize), &'static str> {
    let (&tag, rest) = bytes.split_first().ok_or("a key ends before its tag")?;
    let fixed = |rest: &[u8]| -> Result<u64, &'static str> {
        rest.first_chunk::<8>()
            .map(|bytes| u64::from_be_bytes(*bytes))
            .ok_or("a key ends inside a number")
    };

    match tag {
        NULL => Ok((Value::Null, 1)),
        FALSE => Ok((Value::Bool(false), 1)),
        TRUE => Ok((Value::Bool(true), 1)),
        INT..=INT_LAST => {
            let (value, used) = read_int(tag, rest)?;

            Ok((Value::Int(value), 1 + used))
        }
        FLOAT => Ok((Value::Float(unordered_float(fixed(rest)?)), 9)),
        STRING => {
            let (bytes, used) = unescape(rest)?;
            let text = String::from_utf8(bytes).map_err(|_| "a string key is not UTF-8")?;

            Ok((Value::String(text), 1 + used))
        }
        BYTES => {
            let (bytes, used) = unescape(rest)?;

            Ok((Value::Bytes(bytes), 1 + used))
        }
        _ => Err("a key has an unknown tag"),
    }
}

/// How many bytes the key encoding at the start of `bytes` takes, found
/// without decoding the value, for a caller that needs only where it ends.
pub(crate) fn length(bytes: &[u8]) -> Result<usize, &'static str> {
    let (&tag, rest) = bytes.split_first().ok_or("a key ends before its tag")?;

    match tag {
        NULL | FALSE | TRUE => Ok(1),
        // The tag alone says how long an int is, so its bytes are left to
        // where they are read as an int. A primary key in an index entry
        // written in more bytes than it needs names no object, which the
        // query reports as damage.
        INT..=INT_LAST => {
            let used = usize::from(tag.abs_diff(ZERO));

            if rest.len() >= used {
                Ok(1 + used)
            } else {
                Err("a key ends inside an int")
            }
        }
        FLOAT if rest.len() >= 8 => Ok(9),
        FLOAT => Err("a key ends inside a number"),
        STRING | BYTES => escaped_length(rest).map(|used| 1 + used),
        _ => Err("a key has an unknown tag"),
    }
}

/// How many bytes an escaped string at the start of `bytes` takes, its end
/// included, as [`unescape`] would count them.
fn escaped_length(bytes: &[u8]) -> Result<usize, &'static str> {
    let mut index = 0;

    loop {
        let zero = bytes[index..]
            .iter()
            .position(|&byte| byte == 0)
            .ok_or("a key ends inside a string")?;

        index += zero;

        match bytes.get(index + 1) {
            Some(0) => return Ok(index + 2),
            Some(0xFF) => index += 2,
            Some(_) => return Err("a key has a zero byte out of place"),
            None => return Err("a key ends inside a string"),
        }
    }
}

/// Compares two scalar values as their encodings compare: null first, then
/// by type in the order of the tags, then by value, with floats in the
/// canonical order. A list or an object sorts after everything, though no
/// caller compares one.
pub(crate) fn compare(a: &Value, b: &Value) -> Ordering {
    fn rank(value: &Value) -> u8 {
        match value {
            Value::Null => NULL,
            Value::Bool(false) => FALSE,
            Value::Bool(true) => TRUE,
            Value::Int(_) => INT,
            Value::Float(_) => FLOAT,
            Value::String(_) => STRING,
            Value::Bytes(_) => BYTES,
            Value::List(_) | Value::Object(_) => u8::MAX,
        }
    }

    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => ordered_float(*a).cmp(&ordered_float(*b)),
        (Value::String(a), Value::String(b)) => a.as_bytes().cmp(b.as_bytes()),
        (Value::Bytes(a), Value::Bytes(b)) => a.cmp(b),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// The bytes that the encoding of every string starting with `prefix` starts
/// with, and no other encoding does: the tag and the escaped prefix, without
/// the end.
pub(crate) fn string_prefix(prefix: &str) -> Vec<u8> {
    let mut out = Vec::new();

    escape(STRING, prefix.as_bytes(), &mut out);
    out.truncate(out.len() - 2);

    out
}

/// The first byte string after every string that starts with `prefix`, or
/// `None` if there is none: `prefix` with its trailing `0xFF` bytes dropped
/// and its last byte raised.
pub(crate) fn after_prefix(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut out = prefix.to_vec();

    while let Some(last) = out.pop() {
        if last < u8::MAX {
            out.push(last + 1);

            return Some(out);
        }
    }

    None
}

/// `value` with `-0.0` made `0.0` and every NaN made one NaN, so that values
/// that compare equal encode alike.
pub(crate) fn canonical_float(value: f64) -> f64 {
    if value.is_nan() {
        f64::from_bits(CANONICAL_NAN)
    } else if value == 0.0 {
        0.0
    } else {
        value
    }
}

/// The bits of `value`, canonical, arranged to order as unsigned integers the
/// way the numbers order: negative numbers with every bit flipped, the others
/// with the sign bit set.
fn ordered_float(value: f64) -> u64 {
    let bits = canonical_float(value).to_bits();

    if bits & SIGN == 0 { bits | SIGN } else { !bits }
}

fn unordered_float(bits: u64) -> f64 {
    f64::from_bits(if bits & SIGN == 0 { !bits } else { bits ^ SIGN })
}

/// Appends `tag` and `number` as eight big-endian bytes.
fn number(tag: u8, number: u64, out: &mut Vec<u8>) {
    let mut bytes = [tag; 9];

    bytes[1..].copy_from_slice(&number.to_be_bytes());
    out.extend_from_slice(&bytes);
}

/// Appends `value` in the fewest big-endian bytes of its two's complement
/// that hold it, after the tag that says how many. A negative int takes the
/// bytes that hold its complement, `-1 - value`, and one at least, so that
/// `-1` to `-256` take one byte and more negative ints sort before them,
/// with lower tags.
///
/// Eight bytes for every int made the key of an entry in an index on a small
/// int, with an int primary key, 18 bytes, mostly zeros: a leaf held about
/// half the entries it holds now, and the heads of a branch's keys, the four
/// bytes after the prefix they share, were those zeros.
#[expect(
    clippy::cast_possible_truncation,
    reason = "an int takes at most eight bytes"
)]
fn int(value: i64, out: &mut Vec<u8>) {
    let negative = value < 0;
    let magnitude = if negative { !value } else { value };
    // A negative int takes one byte at least, which tells it from zero.
    let used = (64 - magnitude.leading_zeros() as usize)
        .div_ceil(8)
        .max(usize::from(negative));
    let tag = if negative {
        ZERO - used as u8
    } else {
        ZERO + used as u8
    };
    // The bytes kept are moved to the front and all nine appended, then the
    // rest cut off: a copy of a length known only here was a call to copy
    // memory, which cost a lookup by an int key more than the rest of
    // encoding it.
    let kept = u64::from_be_bytes(value.to_be_bytes())
        .checked_shl(8 * (8 - used) as u32)
        .unwrap_or(0);
    let mut bytes = [tag; 9];
    let start = out.len();

    bytes[1..].copy_from_slice(&kept.to_be_bytes());
    out.extend_from_slice(&bytes);
    out.truncate(start + 1 + used);
}

/// Reads the bytes of an int whose tag is `tag` at the start of `rest`, as
/// [`int`] wrote them. Returns the int and how many bytes it took.
///
/// An int written in more bytes than it needs is refused, so that every int
/// has one encoding and two keys of one value are one key.
fn read_int(tag: u8, rest: &[u8]) -> Result<(i64, usize), &'static str> {
    let negative = tag < ZERO;
    let used = usize::from(if negative { ZERO - tag } else { tag - ZERO });
    let bytes = rest.get(..used).ok_or("a key ends inside an int")?;
    let mut full = [if negative { 0xFF } else { 0 }; 8];

    full[8 - used..].copy_from_slice(bytes);

    let value = i64::from_be_bytes(full);
    let magnitude = if negative { !value } else { value };
    let needed = (64 - magnitude.leading_zeros() as usize).div_ceil(8);

    if (negative != (value < 0)) || needed.max(usize::from(negative)) != used {
        return Err("a key holds an int in more bytes than it needs");
    }

    Ok((value, used))
}

/// Appends `tag` and `bytes` escaped: every `0x00` doubled as `0x00 0xFF`,
/// then `0x00 0x00`. The room is reserved first and the bytes between zeros
/// copied in runs: a key built a byte at a time grew its buffer again and
/// again, which cost a lookup by a string more than the lookup's search of
/// a tree.
fn escape(tag: u8, bytes: &[u8], out: &mut Vec<u8>) {
    let mut rest = bytes;

    out.reserve(bytes.len() + 3);
    out.push(tag);

    while let Some(zero) = rest.iter().position(|&byte| byte == 0) {
        out.extend_from_slice(&rest[..=zero]);
        out.push(0xFF);
        rest = &rest[zero + 1..];
    }

    out.extend_from_slice(rest);
    out.extend_from_slice(&[0, 0]);
}

/// Reads what [`escape`] wrote. Returns the bytes and how many it took.
fn unescape(bytes: &[u8]) -> Result<(Vec<u8>, usize), &'static str> {
    let mut out = Vec::new();
    let mut index = 0;

    loop {
        match (bytes.get(index), bytes.get(index + 1)) {
            (Some(0), Some(0)) => return Ok((out, index + 2)),
            (Some(0), Some(0xFF)) => {
                out.push(0);
                index += 2;
            }
            (Some(0), _) => return Err("a key has a zero byte out of place"),
            (Some(&byte), _) => {
                out.push(byte);
                index += 1;
            }
            (None, _) => return Err("a key ends inside a string"),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::testing::Rng;

    /// The order the encoding has to keep, for values of one type.
    fn order(a: &Value, b: &Value) -> Ordering {
        match (a, b) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => Ordering::Less,
            (_, Value::Null) => Ordering::Greater,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => {
                canonical_float(*a).total_cmp(&canonical_float(*b))
            }
            (Value::String(a), Value::String(b)) => a.as_bytes().cmp(b.as_bytes()),
            (Value::Bytes(a), Value::Bytes(b)) => a.cmp(b),
            _ => unreachable!("values of different types"),
        }
    }

    fn random(rng: &mut Rng, kind: u64) -> Value {
        let edge = rng.below(4) == 0;

        match (kind, rng.below(8)) {
            (_, 0) => Value::Null,
            (0, _) => Value::Bool(rng.below(2) == 1),
            (1, _) if edge => [i64::MIN, -1, 0, 1, i64::MAX][rng.index(5)].into(),
            (1, _) => Value::Int(i64::from_be_bytes(rng.next_u64().to_be_bytes()) >> rng.below(64)),
            (2, _) if edge => [
                f64::NEG_INFINITY,
                -1.0,
                -0.0,
                0.0,
                f64::MIN_POSITIVE,
                1.0,
                f64::INFINITY,
                f64::NAN,
                -f64::NAN,
            ][rng.index(9)]
            .into(),
            (2, _) => Value::Float(f64::from_bits(rng.next_u64())),
            (3, _) => {
                let text: String = (0..rng.index(6))
                    .map(|_| ['\0', 'a', 'b', 'é', '\u{10FFFF}'][rng.index(5)])
                    .collect();

                Value::String(text)
            }
            _ => Value::Bytes(
                (0..rng.index(6))
                    .map(|_| [0, 1, 0xFF][rng.index(3)])
                    .collect(),
            ),
        }
    }

    #[test]
    fn encodings_order_as_the_values_do() {
        let mut rng = Rng::new(3);

        for kind in 0..5 {
            for _ in 0..4000 {
                let (a, b) = (random(&mut rng, kind), random(&mut rng, kind));

                assert_eq!(
                    encoded(&a).unwrap().cmp(&encoded(&b).unwrap()),
                    order(&a, &b),
                    "{a:?} against {b:?}"
                );
            }
        }
    }

    /// A scalar encoded where a record holds it gives the encoding of the
    /// value it reads as.
    #[test]
    fn a_scalar_in_a_record_encodes_as_its_value() {
        let mut rng = Rng::new(6);

        for kind in 0..5 {
            for _ in 0..4000 {
                let value = random(&mut rng, kind);
                let found = match &value {
                    Value::Null => continue,
                    Value::Bool(value) => FieldRef::Bool(*value),
                    Value::Int(value) => FieldRef::Int(*value),
                    Value::Float(value) => FieldRef::Float(*value),
                    Value::String(text) => FieldRef::String(text.as_bytes()),
                    Value::Bytes(bytes) => FieldRef::Bytes(bytes),
                    Value::List(_) | Value::Object(_) => unreachable!(),
                };
                let mut out = vec![0xAA];

                encode_field(found, &mut out).unwrap();
                assert_eq!(out[1..], encoded(&value).unwrap(), "{value:?}");
            }
        }

        assert!(encode_field(FieldRef::Encoded(&[0x08]), &mut Vec::new()).is_err());
    }

    #[test]
    fn concatenations_order_field_by_field() {
        let mut rng = Rng::new(4);

        for _ in 0..4000 {
            let (kind_a, kind_b) = (rng.below(5), rng.below(5));
            let first = (random(&mut rng, kind_a), random(&mut rng, kind_b));
            let second = (random(&mut rng, kind_a), random(&mut rng, kind_b));
            let join = |(a, b): &(Value, Value)| {
                let mut out = encoded(a).unwrap();

                encode(b, &mut out).unwrap();
                out
            };

            assert_eq!(
                join(&first).cmp(&join(&second)),
                order(&first.0, &second.0).then(order(&first.1, &second.1)),
                "{first:?} against {second:?}"
            );
        }
    }

    #[test]
    fn a_key_reads_back_as_its_value() {
        let mut rng = Rng::new(5);

        for _ in 0..4000 {
            let kind = rng.below(5);
            let value = random(&mut rng, kind);
            let mut bytes = encoded(&value).unwrap();

            bytes.extend_from_slice(b"rest");

            let (read, used) = decode(&bytes).unwrap();

            assert_eq!(order(&read, &value), Ordering::Equal, "{value:?}");
            assert_eq!(&bytes[used..], b"rest");
            assert_eq!(length(&bytes), Ok(used), "{value:?}");
        }
    }

    #[test]
    fn the_documented_layout_is_the_one_written() {
        assert_eq!(encoded(&Value::Null).unwrap(), [0x01]);
        assert_eq!(encoded(&Value::Int(0)).unwrap(), [0x0C]);
        assert_eq!(encoded(&Value::Int(42)).unwrap(), [0x0D, 42]);
        assert_eq!(encoded(&Value::Int(300)).unwrap(), [0x0E, 0x01, 0x2C]);
        assert_eq!(encoded(&Value::Int(-1)).unwrap(), [0x0B, 0xFF]);
        assert_eq!(encoded(&Value::Int(-257)).unwrap(), [0x0A, 0xFE, 0xFF]);
        assert_eq!(
            encoded(&Value::Int(i64::MIN)).unwrap(),
            [0x04, 0x80, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            encoded(&Value::Int(i64::MAX)).unwrap(),
            [0x14, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            encoded(&Value::Float(1.0)).unwrap(),
            [0x15, 0xBF, 0xF0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            encoded(&Value::String("a\0".into())).unwrap(),
            [0x16, b'a', 0, 0xFF, 0, 0]
        );
        assert_eq!(encoded(&Value::Bytes(vec![1])).unwrap(), [0x17, 1, 0, 0]);
    }

    /// An int takes one byte for its tag and the fewest that hold it, on
    /// either side of every length's bounds.
    #[test]
    fn an_int_takes_the_fewest_bytes_that_hold_it() {
        assert_eq!(encoded(&Value::Int(0)).unwrap().len(), 1);

        for used in 1..=8u32 {
            let largest = if used == 8 {
                i64::MAX
            } else {
                (1 << (8 * used)) - 1
            };
            let smallest = if used == 8 {
                i64::MIN
            } else {
                -(1 << (8 * used))
            };

            for (value, expected) in [
                (largest, used),
                (smallest, used),
                (largest.checked_add(1).unwrap_or(largest), used.min(7) + 1),
                (smallest.checked_sub(1).unwrap_or(smallest), used.min(7) + 1),
                (1 << (8 * (used - 1)), used),
                (-(1 << (8 * (used - 1))) - 1, used),
            ] {
                let bytes = encoded(&Value::Int(value)).unwrap();

                assert_eq!(bytes.len(), 1 + expected as usize, "{value}");
                assert_eq!(decode(&bytes), Ok((Value::Int(value), bytes.len())));
            }
        }
    }

    #[test]
    fn damaged_keys_are_refused() {
        for bytes in [
            &[][..],
            &[0x0E, 1],
            &[0x04, 1, 2],
            &[STRING, b'a'],
            &[STRING, 0, 1],
            &[BYTES + 1],
            &[0x00],
        ] {
            assert!(decode(bytes).is_err(), "{bytes:?}");
            assert!(length(bytes).is_err(), "{bytes:?}");
        }
    }

    /// An int in more bytes than it needs would be a second key for its
    /// value, and so would one past the range of its length.
    #[test]
    fn an_int_in_more_bytes_than_it_needs_is_refused() {
        for bytes in [
            &[0x0D, 0][..],
            &[0x0E, 0, 0xFF],
            &[0x0A, 0xFF, 0],
            &[0x14, 0x80, 0, 0, 0, 0, 0, 0, 0],
            &[0x04, 0x7F, 0, 0, 0, 0, 0, 0, 0],
        ] {
            assert_eq!(
                decode(bytes),
                Err("a key holds an int in more bytes than it needs"),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn values_compare_as_their_encodings_do() {
        let mut rng = Rng::new(21);
        let samples: Vec<Value> = (0..400)
            .map(|_| {
                let kind = rng.below(5);

                random(&mut rng, kind)
            })
            .collect();

        for a in &samples {
            for b in &samples {
                assert_eq!(
                    compare(a, b),
                    encoded(a).unwrap().cmp(&encoded(b).unwrap()),
                    "{a:?} and {b:?}"
                );
            }
        }
    }

    #[test]
    fn a_string_prefix_bounds_exactly_the_strings_that_start_with_it() {
        let words = ["", "a", "a\0", "a\0b", "ab", "b", "\0", "\u{ff}", "a\u{ff}"];

        for prefix in words {
            let start = string_prefix(prefix);
            let end = after_prefix(&start);

            for word in words {
                let key = encoded(&Value::from(word)).unwrap();
                let inside = key >= start && end.as_ref().is_none_or(|end| key < *end);

                assert_eq!(inside, word.starts_with(prefix), "{word:?} and {prefix:?}");
            }
        }

        assert_eq!(after_prefix(&[1, 0xFF, 0xFF]), Some(vec![2]));
        assert_eq!(after_prefix(&[0xFF]), None);
    }
}
