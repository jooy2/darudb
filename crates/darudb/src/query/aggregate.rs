//! Aggregates: a value worked out from the objects a query finds rather than
//! the objects themselves (`design/objects.md`, "Aggregates").
//!
//! Sums and means are exact until the end. Ints add up in an `i128`, and
//! floats, with the ints a mean reads, in [`Exact`], a fixed-point number
//! wide enough to hold any sum of doubles without rounding, which is rounded
//! once when the result is made. So the answer does not depend on the order
//! the objects are read in, and every plan of a query gives the same one,
//! which is what the tests compare.

use crate::format::object::Value;
use crate::format::object::codec::{self, Raw};

/// What to work out from the values of one field of the objects a query
/// finds, for [`CollectionReader::aggregate`](crate::CollectionReader::aggregate).
///
/// Null values, and paths that reach no value, are left out. A sum of no
/// values is zero, and the least, the greatest and the mean of no values are
/// null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Aggregate {
    /// The sum of an int or float field: an int for an int field, exact, and
    /// a float for a float field, the exact sum rounded once.
    Sum,
    /// The least value of an int, float, string or bytes field, in the order
    /// a sort gives.
    Min,
    /// The greatest value, as [`Min`](Self::Min) gives the least.
    Max,
    /// The mean of an int or float field, as a float: the exact sum divided
    /// by the number of values, rounded once.
    Avg,
}

impl Aggregate {
    /// The function's code in the IR (`design/objects.md`, "The IR").
    pub fn code(self) -> i64 {
        match self {
            Aggregate::Sum => 1,
            Aggregate::Min => 2,
            Aggregate::Max => 3,
            Aggregate::Avg => 4,
        }
    }

    /// The function whose code in the IR is `code`, if any is.
    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            1 => Aggregate::Sum,
            2 => Aggregate::Min,
            3 => Aggregate::Max,
            4 => Aggregate::Avg,
            _ => return None,
        })
    }

    /// The function as an error names it.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Aggregate::Sum => "sum",
            Aggregate::Min => "min",
            Aggregate::Max => "max",
            Aggregate::Avg => "avg",
        }
    }
}

/// How many 64-bit words [`Exact`] holds. A finite double is an integer of
/// at most 53 bits times a power of two from 2^-1074 to 2^971, so its bits
/// lie between 2^-1074 and 2^1023: 2098 bits, with 64 more so that adding up
/// to 2^64 of the largest cannot overflow, and the sign.
const WORDS: usize = 34;

/// The bit of [`Exact`] that stands for 1: bit 0 stands for 2^-1074, the
/// least a double holds.
const ONE: u32 = 1074;

/// An exact sum of doubles and ints, as a two's complement fixed-point number
/// whose bit `n` stands for 2^(n - 1074), and the infinities and NaNs added,
/// which are kept apart from it.
#[derive(Clone)]
pub(crate) struct Exact {
    words: [u64; WORDS],
    nan: bool,
    positive_infinity: bool,
    negative_infinity: bool,
}

impl Default for Exact {
    fn default() -> Self {
        Self {
            words: [0; WORDS],
            nan: false,
            positive_infinity: false,
            negative_infinity: false,
        }
    }
}

impl Exact {
    /// Adds `value`.
    pub(crate) fn add_float(&mut self, value: f64) {
        if value.is_nan() {
            self.nan = true;

            return;
        }

        if value.is_infinite() {
            if value > 0.0 {
                self.positive_infinity = true;
            } else {
                self.negative_infinity = true;
            }

            return;
        }

        let bits = value.to_bits();
        let negative = bits >> 63 != 0;
        let exponent = ((bits >> 52) & 0x7FF) as u32;
        let fraction = bits & ((1 << 52) - 1);
        // A subnormal's fraction counts units of 2^-1074; a normal number's
        // has its leading one added and counts units of 2^(exponent - 1075).
        let (magnitude, shift) = if exponent == 0 {
            (fraction, 0)
        } else {
            (fraction | 1 << 52, exponent - 1)
        };

        if magnitude != 0 {
            self.add_shifted(magnitude, shift, negative);
        }
    }

    /// Adds `value`, exactly.
    pub(crate) fn add_int(&mut self, value: i64) {
        if value != 0 {
            self.add_shifted(value.unsigned_abs(), ONE, value < 0);
        }
    }

    /// Adds or takes away `magnitude` times 2^(shift - 1074).
    fn add_shifted(&mut self, magnitude: u64, shift: u32, negative: bool) {
        let word = (shift / 64) as usize;
        let offset = shift % 64;
        let low = magnitude << offset;
        let high = if offset == 0 {
            0
        } else {
            magnitude >> (64 - offset)
        };

        if negative {
            let mut borrow = false;

            for (at, part) in [(word, low), (word + 1, high)] {
                let (difference, under) = self.words[at].overflowing_sub(part);
                let (difference, under_again) = difference.overflowing_sub(u64::from(borrow));

                self.words[at] = difference;
                borrow = under || under_again;
            }

            let mut at = word + 2;

            while borrow && at < WORDS {
                let (difference, under) = self.words[at].overflowing_sub(1);

                self.words[at] = difference;
                borrow = under;
                at += 1;
            }
        } else {
            let mut carry = false;

            for (at, part) in [(word, low), (word + 1, high)] {
                let (sum, over) = self.words[at].overflowing_add(part);
                let (sum, over_again) = sum.overflowing_add(u64::from(carry));

                self.words[at] = sum;
                carry = over || over_again;
            }

            let mut at = word + 2;

            while carry && at < WORDS {
                let (sum, over) = self.words[at].overflowing_add(1);

                self.words[at] = sum;
                carry = over;
                at += 1;
            }
        }
    }

    /// The sum divided by `divisor`, which is not zero, rounded once to the
    /// nearest double, ties to even. Infinities and NaNs added decide it
    /// alone: a NaN, or infinities of both signs, make a NaN.
    pub(crate) fn rounded(&self, divisor: u64) -> f64 {
        debug_assert!(divisor != 0, "a mean of no values is null, not a division");

        if self.nan || (self.positive_infinity && self.negative_infinity) {
            return f64::NAN;
        }

        if self.positive_infinity {
            return f64::INFINITY;
        }

        if self.negative_infinity {
            return f64::NEG_INFINITY;
        }

        let negative = self.words[WORDS - 1] >> 63 != 0;
        let mut words = self.words;

        if negative {
            // Two's complement: every bit flipped, and one added.
            let mut carry = true;

            for word in &mut words {
                let (sum, over) = (!*word).overflowing_add(u64::from(carry));

                *word = sum;
                carry = over;
            }
        }

        // Long division, a word at a time from the top; what is left over
        // only says whether the quotient was cut short.
        let mut remainder = 0u128;

        if divisor != 1 {
            for word in words.iter_mut().rev() {
                let dividend = remainder << 64 | u128::from(*word);

                // The remainder is less than the divisor, so the quotient of
                // each word fits in a word.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "less than 2^64, as said above"
                )]
                let quotient = (dividend / u128::from(divisor)) as u64;

                *word = quotient;
                remainder = dividend % u128::from(divisor);
            }
        }

        let magnitude = round_quotient(&words, remainder, u128::from(divisor));

        if negative { -magnitude } else { magnitude }
    }
}

/// The double nearest the fixed-point number `words`, plus `remainder` over
/// `divisor` of its last bit, ties to even.
fn round_quotient(words: &[u64; WORDS], remainder: u128, divisor: u128) -> f64 {
    let Some(top) = highest_bit(words) else {
        // Less than the least subnormal: zero, or that subnormal if what is
        // left is more than half of it.
        return f64::from_bits(u64::from(remainder * 2 > divisor));
    };

    // How the part cut off compares with half of the last bit kept.
    let half = |cut: u32| -> std::cmp::Ordering {
        if cut == 0 {
            return (remainder * 2).cmp(&divisor);
        }

        let guard = bit(words, cut - 1);
        let sticky = remainder != 0 || (0..cut - 1).any(|at| bit(words, at));

        match (guard, sticky) {
            (false, _) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Equal,
            (true, true) => std::cmp::Ordering::Greater,
        }
    };

    if top <= 52 {
        // A subnormal or the least normal numbers: the bits are the double's
        // own, and one more after rounding up is still the right double.
        let kept = words[0];
        let up = match half(0) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => kept & 1 == 1,
            std::cmp::Ordering::Less => false,
        };

        return f64::from_bits(kept + u64::from(up));
    }

    let mut cut = top - 52;
    let mut kept = bits_from(words, cut);
    let up = match half(cut) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Equal => kept & 1 == 1,
        std::cmp::Ordering::Less => false,
    };

    if up {
        kept += 1;

        if kept == 1 << 53 {
            kept >>= 1;
            cut += 1;
        }
    }

    // `kept` times 2^(cut - 1074), with its leading one at bit 52: the
    // exponent field is `cut + 1`.
    let exponent = u64::from(cut) + 1;

    if exponent >= 0x7FF {
        return f64::INFINITY;
    }

    f64::from_bits(exponent << 52 | (kept & ((1 << 52) - 1)))
}

/// The position of the highest bit set in `words`, if any is.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a word's position among the 34"
)]
fn highest_bit(words: &[u64; WORDS]) -> Option<u32> {
    (0..WORDS)
        .rev()
        .find(|at| words[*at] != 0)
        .map(|at| at as u32 * 64 + 63 - words[at].leading_zeros())
}

fn bit(words: &[u64; WORDS], at: u32) -> bool {
    words[(at / 64) as usize] >> (at % 64) & 1 == 1
}

/// The 53 bits of `words` from bit `from` up.
fn bits_from(words: &[u64; WORDS], from: u32) -> u64 {
    let word = (from / 64) as usize;
    let offset = from % 64;
    let low = words[word] >> offset;
    let high = match (offset, words.get(word + 1)) {
        (0, _) | (_, None) => 0,
        (_, Some(next)) => next << (64 - offset),
    };

    (low | high) & ((1 << 53) - 1)
}

/// `value`, an aggregate's result, as a language binding reads it: its tag
/// and payload as a record holds a value, and null as the tag `0x01`, which
/// changes give a null field (`design/objects.md`, "Records").
pub fn tagged(value: &Value) -> Vec<u8> {
    let raw = match value {
        Value::Bool(value) => Raw::Bool(*value),
        Value::Int(value) => Raw::Int(*value),
        Value::Float(value) => Raw::Float(*value),
        Value::String(value) => Raw::String(value.clone()),
        Value::Bytes(value) => Raw::Bytes(value.clone()),
        // No aggregate gives a list or an object.
        Value::Null | Value::List(_) | Value::Object(_) => return vec![NULL_TAG],
    };
    let mut out = Vec::new();

    codec::write_value(&raw, &mut out);
    out
}

/// The tag of a null in changes, which [`tagged`] gives a null aggregate.
const NULL_TAG: u8 = 0x01;

/// The value an aggregate of `function` over no values gives, for a field
/// whose values are `float` or not.
pub(crate) fn of_nothing(function: Aggregate, float: bool) -> Value {
    match (function, float) {
        (Aggregate::Sum, true) => Value::Float(0.0),
        (Aggregate::Sum, false) => Value::Int(0),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(values: &[f64]) -> f64 {
        let mut exact = Exact::default();

        for value in values {
            exact.add_float(*value);
        }

        exact.rounded(1)
    }

    fn mean(values: &[f64]) -> f64 {
        let mut exact = Exact::default();

        for value in values {
            exact.add_float(*value);
        }

        exact.rounded(values.len() as u64)
    }

    #[test]
    fn sums_are_exact_until_rounded_once() {
        assert_eq!(sum(&[0.1; 10]), 1.0);
        assert_eq!(sum(&[1e100, 1.0, -1e100]), 1.0);
        assert_eq!(sum(&[1e308, 1e308, -1e308]), 1e308);
        assert_eq!(sum(&[f64::MAX, f64::MAX]), f64::INFINITY);
        assert_eq!(sum(&[-f64::MAX, -f64::MAX]), f64::NEG_INFINITY);
        assert_eq!(sum(&[]), 0.0);
        assert_eq!(sum(&[-0.0]).to_bits(), 0.0f64.to_bits());
        assert_eq!(sum(&[5e-324, 5e-324]), 1e-323);
        assert_eq!(sum(&[5e-324, -5e-324]), 0.0);
        assert_eq!(sum(&[2.5, -0.5]), 2.0);
        assert_eq!(sum(&[1.0, 2f64.powi(-53)]), 1.0, "a tie rounds to even");
        assert_eq!(
            sum(&[1.0, 2f64.powi(-53), 2f64.powi(-80)]),
            1.0 + f64::EPSILON,
            "past the tie rounds up"
        );
        assert_eq!(
            sum(&[1.0 + f64::EPSILON, 2f64.powi(-53)]),
            1.0 + 2.0 * f64::EPSILON,
            "a tie on an odd last bit rounds up"
        );
        assert!(sum(&[f64::NAN, 1.0]).is_nan());
        assert!(sum(&[f64::INFINITY, f64::NEG_INFINITY]).is_nan());
        assert_eq!(sum(&[f64::INFINITY, -f64::MAX]), f64::INFINITY);
    }

    #[test]
    fn sums_do_not_depend_on_the_order() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut values = Vec::new();

        for _ in 0..500 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;

            let exponent = (state % 120) as i32 - 60;
            let sign = if state & 1 == 0 { 1.0 } else { -1.0 };

            values.push(sign * (state >> 11) as f64 * 2f64.powi(exponent - 53));
        }

        let forwards = sum(&values);

        values.reverse();
        assert_eq!(sum(&values).to_bits(), forwards.to_bits());
        values.sort_by(f64::total_cmp);
        assert_eq!(sum(&values).to_bits(), forwards.to_bits());
    }

    #[test]
    fn means_round_once() {
        assert_eq!(mean(&[1.0, 2.0]), 1.5);
        assert_eq!(mean(&[1e308, 1e308]), 1e308, "the sum alone would overflow");
        assert_eq!(mean(&[1.0, 1.0, 1.0]), 1.0);
        assert_eq!(mean(&[0.0, 0.0, 1.0]), 1.0 / 3.0);
        assert_eq!(mean(&[-1.0, -2.0]), -1.5);
        assert_eq!(
            mean(&[5e-324, 0.0]),
            0.0,
            "half the least subnormal ties to zero"
        );
        assert_eq!(mean(&[5e-324, 5e-324, 5e-324, 0.0]), 5e-324);

        let mut exact = Exact::default();

        for value in [i64::MAX, i64::MAX, 1] {
            exact.add_int(value);
        }

        assert_eq!(exact.rounded(3), 6_148_914_691_236_517_205.0);

        let mut exact = Exact::default();

        exact.add_int(i64::MIN);
        exact.add_int(-1);
        assert_eq!(exact.rounded(1), -9_223_372_036_854_775_809.0);
    }

    #[test]
    fn results_cross_as_tagged_values() {
        assert_eq!(tagged(&Value::Null), [0x01]);
        assert_eq!(tagged(&Value::Int(-1)), [0x04, 0x01]);
        assert_eq!(tagged(&Value::from("a")), [0x06, 1, b'a']);
        assert_eq!(tagged(&Value::Bytes(vec![7])), [0x07, 1, 7]);

        let mut float = vec![0x05];

        float.extend_from_slice(&1.5f64.to_le_bytes());
        assert_eq!(tagged(&Value::Float(1.5)), float);
    }

    #[test]
    fn codes_round_trip() {
        for function in [
            Aggregate::Sum,
            Aggregate::Min,
            Aggregate::Max,
            Aggregate::Avg,
        ] {
            assert_eq!(Aggregate::from_code(function.code()), Some(function));
        }

        assert_eq!(Aggregate::from_code(0), None);
        assert_eq!(Aggregate::from_code(5), None);
    }
}
