//! Values as keys: bytes that order as the values do.
//!
//! The kernel orders keys as unsigned bytes and nothing else, so a primary key
//! or an indexed value is encoded to keep its order in its bytes
//! (`design/objects.md`, "Keys"). Each encoding is a tag and the value, and
//! ends where it can be told to end, so encodings concatenate: the encoding of
//! a value followed by a primary key sorts by the value, then the key.

use super::value::Value;

pub(crate) const NULL: u8 = 0x01;
const FALSE: u8 = 0x02;
const TRUE: u8 = 0x03;
const INT: u8 = 0x04;
const FLOAT: u8 = 0x05;
const STRING: u8 = 0x06;
const BYTES: u8 = 0x07;

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
        Value::Int(value) => {
            out.push(INT);
            out.extend_from_slice(&(u64::from_be_bytes(value.to_be_bytes()) ^ SIGN).to_be_bytes());
        }
        Value::Float(value) => {
            out.push(FLOAT);
            out.extend_from_slice(&ordered_float(*value).to_be_bytes());
        }
        Value::String(value) => {
            out.push(STRING);
            escape(value.as_bytes(), out);
        }
        Value::Bytes(value) => {
            out.push(BYTES);
            escape(value, out);
        }
        Value::List(_) | Value::Object(_) => return Err("a list or an object is not a key"),
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
        INT => Ok((
            Value::Int(i64::from_be_bytes((fixed(rest)? ^ SIGN).to_be_bytes())),
            9,
        )),
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

/// Writes `bytes` with every `0x00` doubled as `0x00 0xFF`, then `0x00 0x00`.
fn escape(bytes: &[u8], out: &mut Vec<u8>) {
    for &byte in bytes {
        out.push(byte);

        if byte == 0 {
            out.push(0xFF);
        }
    }

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
    use std::cmp::Ordering;

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
        }
    }

    #[test]
    fn the_documented_layout_is_the_one_written() {
        assert_eq!(encoded(&Value::Null).unwrap(), [0x01]);
        assert_eq!(
            encoded(&Value::Int(-1)).unwrap(),
            [0x04, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            encoded(&Value::Float(1.0)).unwrap(),
            [0x05, 0xBF, 0xF0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            encoded(&Value::String("a\0".into())).unwrap(),
            [0x06, b'a', 0, 0xFF, 0, 0]
        );
    }

    #[test]
    fn damaged_keys_are_refused() {
        for bytes in [
            &[][..],
            &[0x04, 1, 2],
            &[0x06, b'a'],
            &[0x06, 0, 1],
            &[0x09],
        ] {
            assert!(decode(bytes).is_err(), "{bytes:?}");
        }
    }
}
