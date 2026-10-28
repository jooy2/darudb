//! Sets of primary keys: what a part of a filter that indexes answer alone
//! comes to (`design/objects.md`, "Sets of keys").
//!
//! A set holds encoded primary keys in one buffer, without repeats, and a
//! table of their hashes, so that a walk asks whether a key is in it without
//! searching. When every key is an int and they lie close together, as the
//! numbers of an auto-increment collection do, it holds a bit for each int
//! between the least and the greatest instead: a walk that asked the table
//! for each of 150,000 keys spent more than two fifths of a count there, at
//! three places in memory a key. It is sorted only when its objects are read
//! in the collection's order: a count or a meeting with another set needs no
//! order, and sorting a set of thousands of keys took most of a count's
//! time. Keys come from the file, which is untrusted: a crafted file can
//! make them collide and slow a lookup down, but a collision is resolved by
//! comparing the keys, so it cannot change an answer.

use crate::format::object::key;

/// Encoded primary keys, gathered, then looked up, and sorted when asked.
#[derive(Debug, Default)]
pub(crate) struct KeySet {
    /// The keys one after another.
    bytes: Vec<u8>,
    /// Where each key ends in `bytes`, in the order of the keys.
    ends: Vec<usize>,
    /// How a key is looked up, once the set is sealed.
    lookup: Lookup,
    /// Whether the keys are in ascending order.
    sorted: bool,
}

#[derive(Debug, Default)]
enum Lookup {
    /// Not sealed yet: nothing is in the set.
    #[default]
    None,
    /// For each slot, one more than the position of the key there, or 0 for
    /// none: open addressing over `hash`, a power of two in size.
    Table(Vec<u32>),
    /// A bit for each int from `first` on, set for the ints in the set.
    Bits { first: i64, words: Vec<u64> },
}

/// What [`KeySet::ints`] finds of a set whose keys are all ints.
#[derive(Clone, Copy, Debug)]
struct Ints {
    least: i64,
    greatest: i64,
    sorted: bool,
}

/// How many bits a set of ints may hold for each of its keys, which keeps
/// the bits no larger than a table, of eight to sixteen bytes a key.
const BITS_PER_KEY: i128 = 64;

/// The hash of a key: its words folded together, as the page cache's keys
/// are.
fn hash(key: &[u8]) -> u64 {
    let mut hash = key.len() as u64;

    for chunk in key.chunks(8) {
        let mut word = [0u8; 8];

        word[..chunk.len()].copy_from_slice(chunk);
        hash = (hash.rotate_left(5) ^ u64::from_le_bytes(word)).wrapping_mul(0x517C_C1B7_2722_0A95);
    }

    hash ^ (hash >> 29)
}

/// The slot of the table, of `mask + 1` slots, where the search for `key`
/// starts.
fn slot_of(key: &[u8], mask: usize) -> usize {
    // What is left after the mask fits a `usize`, being at most the mask.
    usize::try_from(hash(key) & mask as u64).unwrap_or(0)
}

impl KeySet {
    /// The keys gathered so far, before [`KeySet::seal`] drops the repeats.
    pub(crate) fn push(&mut self, key: &[u8]) {
        self.bytes.extend_from_slice(key);
        self.ends.push(self.bytes.len());
    }

    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The `at`th key, in the set's order.
    pub(crate) fn get(&self, at: usize) -> &[u8] {
        let start = if at == 0 { 0 } else { self.ends[at - 1] };

        &self.bytes[start..self.ends[at]]
    }

    /// Drops the repeats and makes the lookup, keeping the keys in the
    /// order they were gathered.
    pub(crate) fn seal(mut self) -> Self {
        let mut dropped: Option<Vec<bool>> = None;
        let len = self.len();
        let mut repeat = |at| dropped.get_or_insert_with(|| vec![false; len])[at] = true;

        let ints = self.ints();

        // Ints order as their encodings do.
        self.sorted = match ints {
            Some(Ints { sorted, .. }) => sorted,
            None => (1..len).all(|at| self.get(at - 1) < self.get(at)),
        };
        // Keys in ascending order have no repeats to look for.
        self.lookup = match ints.and_then(|ints| self.bits_of(ints, &mut repeat)) {
            Some(bits) => bits,
            None => Lookup::Table(self.table_of(!self.sorted, &mut repeat)),
        };

        if let Some(dropped) = dropped {
            let order: Vec<usize> = (0..len).filter(|&at| !dropped[at]).collect();

            self.arrange(&order);
        }

        self
    }

    /// The keys in ascending order, as a walk of them in the collection's
    /// order takes them.
    pub(crate) fn in_order(mut self) -> Self {
        if !self.sorted {
            let mut order: Vec<usize> = (0..self.len()).collect();

            order.sort_unstable_by(|&a, &b| self.get(a).cmp(self.get(b)));
            self.arrange(&order);
            self.sorted = true;
        }

        self
    }

    /// Whether `key` is in the set, once it is sealed.
    pub(crate) fn contains(&self, key: &[u8]) -> bool {
        match &self.lookup {
            Lookup::None => false,
            Lookup::Table(table) => {
                let mask = table.len() - 1;
                let mut slot = slot_of(key, mask);

                loop {
                    match table[slot] {
                        0 => return false,
                        at if self.get(at as usize - 1) == key => return true,
                        _ => slot = (slot + 1) & mask,
                    }
                }
            }
            // A key that is not an int written as the engine writes one is
            // not among the set's, which are.
            Lookup::Bits { first, words } => key::whole_int(key)
                .and_then(|value| value.checked_sub(*first))
                .and_then(|bit| usize::try_from(bit).ok())
                .and_then(|bit| words.get(bit / 64).map(|word| word >> (bit % 64) & 1))
                .is_some_and(|set| set == 1),
        }
    }

    /// The least and the greatest of the keys, when every one is an int,
    /// and whether they are in ascending order.
    fn ints(&self) -> Option<Ints> {
        let mut ints = Ints {
            least: i64::MAX,
            greatest: i64::MIN,
            sorted: true,
        };
        let mut last = None;

        for at in 0..self.len() {
            let value = key::whole_int(self.get(at))?;

            ints.least = ints.least.min(value);
            ints.greatest = ints.greatest.max(value);
            ints.sorted &= last.is_none_or(|last| last < value);
            last = Some(value);
        }

        Some(ints)
    }

    /// The bits of the keys, which are `ints`, when there are no more than
    /// [`BITS_PER_KEY`] ints a key between the least and the greatest. It
    /// calls `repeat` with the position of each key that one before it holds
    /// already.
    fn bits_of(&self, ints: Ints, repeat: &mut impl FnMut(usize)) -> Option<Lookup> {
        let span = i128::from(ints.greatest) - i128::from(ints.least) + 1;

        if self.is_empty() || span > BITS_PER_KEY * self.len() as i128 {
            return None;
        }

        // The span fits, being at most 64 times a length.
        let mut words = vec![0u64; usize::try_from(span).ok()?.div_ceil(64)];

        for at in 0..self.len() {
            let value = key::whole_int(self.get(at))?;
            let bit = usize::try_from(i128::from(value) - i128::from(ints.least)).ok()?;
            let word = &mut words[bit / 64];
            let mask = 1 << (bit % 64);

            if *word & mask != 0 {
                repeat(at);
            }

            *word |= mask;
        }

        Some(Lookup::Bits {
            first: ints.least,
            words,
        })
    }

    /// The table of the keys' hashes. With `repeats`, it calls `repeat` with
    /// the position of each key that one before it holds already, which it
    /// leaves out; without, the keys are known to be different, and a key in
    /// a slot is passed over without comparing.
    fn table_of(&self, repeats: bool, repeat: &mut impl FnMut(usize)) -> Vec<u32> {
        let slots = (self.len() * 2).next_power_of_two().max(16);
        let mask = slots - 1;
        let mut table = vec![0u32; slots];

        for at in 0..self.len() {
            let key = self.get(at);
            let mut slot = slot_of(key, mask);

            loop {
                match table[slot] {
                    0 => {
                        // A set holds fewer keys than a collection can,
                        // which is far below `u32::MAX`.
                        table[slot] = u32::try_from(at + 1).unwrap_or(u32::MAX);
                        break;
                    }
                    other if repeats && self.get(other as usize - 1) == key => {
                        repeat(at);
                        break;
                    }
                    _ => slot = (slot + 1) & mask,
                }
            }
        }

        table
    }

    /// Puts the keys at the positions `order` gives in that order, and makes
    /// the table again; the bits of a set of ints do not change.
    fn arrange(&mut self, order: &[usize]) {
        let mut bytes = Vec::with_capacity(self.bytes.len());
        let mut ends = Vec::with_capacity(order.len());

        for &at in order {
            bytes.extend_from_slice(self.get(at));
            ends.push(bytes.len());
        }

        self.bytes = bytes;
        self.ends = ends;

        if let Lookup::Table(_) = self.lookup {
            self.lookup = Lookup::Table(self.table_of(false, &mut |_| {}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KeySet, Lookup};
    use crate::format::object::Value;
    use crate::format::object::key;

    fn int(value: i64) -> Vec<u8> {
        key::encoded(&Value::Int(value)).unwrap()
    }

    fn set(keys: &[&[u8]]) -> KeySet {
        let mut set = KeySet::default();

        for key in keys {
            set.push(key);
        }

        set.seal()
    }

    fn keys(set: &KeySet) -> Vec<Vec<u8>> {
        (0..set.len()).map(|at| set.get(at).to_vec()).collect()
    }

    #[test]
    fn a_set_drops_repeats_finds_its_keys_and_sorts_when_asked() {
        let set = set(&[b"b", b"a", b"", b"ab", b"a", b"b"]);

        assert_eq!(
            keys(&set),
            vec![b"b".to_vec(), b"a".to_vec(), b"".to_vec(), b"ab".to_vec()]
        );

        for key in [b"".as_slice(), b"a", b"ab", b"b"] {
            assert!(set.contains(key));
        }

        for key in [b"c".as_slice(), b"aa", b"ba", b"\0"] {
            assert!(!set.contains(key));
        }

        let set = set.in_order();

        assert_eq!(
            keys(&set),
            vec![b"".to_vec(), b"a".to_vec(), b"ab".to_vec(), b"b".to_vec()]
        );

        for key in [b"".as_slice(), b"a", b"ab", b"b"] {
            assert!(set.contains(key));
        }

        assert!(!KeySet::default().contains(b"a"));
    }

    #[test]
    fn many_keys_are_all_found() {
        let mut set = KeySet::default();

        for n in (0u32..5000).rev() {
            set.push(&n.to_be_bytes());
        }

        let set = set.seal().in_order();

        assert_eq!(set.len(), 5000);

        for n in 0u32..5000 {
            assert!(set.contains(&n.to_be_bytes()));
        }

        assert!(!set.contains(&5000u32.to_be_bytes()));
        assert!((1..set.len()).all(|at| set.get(at - 1) < set.get(at)));
    }

    #[test]
    fn ints_close_together_are_looked_up_by_their_bits() {
        let mut set = KeySet::default();

        for value in [7, -3, 300, 7, 0, 64, -3] {
            set.push(&int(value));
        }

        let set = set.seal();

        assert!(matches!(set.lookup, Lookup::Bits { .. }));
        assert_eq!(
            keys(&set),
            [7, -3, 300, 0, 64].map(int).to_vec(),
            "repeats are dropped and the order kept"
        );

        for value in [7, -3, 300, 0, 64] {
            assert!(set.contains(&int(value)));
        }

        for value in [-4, -2, 1, 63, 65, 299, 301, i64::MIN, i64::MAX] {
            assert!(!set.contains(&int(value)));
        }

        // Seven written in two bytes, an int with a byte after it, a string
        // and nothing are not keys of the set.
        let mut long_seven = int(7);

        long_seven[0] += 1;
        long_seven.insert(1, 0);

        let mut seven_and_more = int(7);

        seven_and_more.push(0);

        for other in [
            long_seven,
            seven_and_more,
            b"\x15a\0\0".to_vec(),
            Vec::new(),
        ] {
            assert!(!set.contains(&other));
        }

        let set = set.in_order();

        assert_eq!(keys(&set), [-3, 0, 7, 64, 300].map(int).to_vec());
        assert!(set.contains(&int(300)) && !set.contains(&int(301)));
    }

    #[test]
    fn ints_far_apart_and_other_keys_are_looked_up_in_a_table() {
        let sealed = set(&[&int(1), &int(1_000)]);

        assert!(matches!(sealed.lookup, Lookup::Table(_)));
        assert!(sealed.contains(&int(1_000)) && !sealed.contains(&int(999)));

        let sealed = set(&[&int(i64::MIN), &int(i64::MAX)]);

        assert!(matches!(sealed.lookup, Lookup::Table(_)));
        assert!(sealed.contains(&int(i64::MIN)) && sealed.contains(&int(i64::MAX)));

        let sealed = set(&[&int(1), b"a".as_slice()]);

        assert!(matches!(sealed.lookup, Lookup::Table(_)));
        assert!(sealed.contains(b"a") && sealed.contains(&int(1)));
    }
}
