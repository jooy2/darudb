//! Sets of primary keys: what a part of a filter that indexes answer alone
//! comes to (`design/objects.md`, "Sets of keys").
//!
//! A set holds encoded primary keys in one buffer, without repeats, and a
//! table of their hashes, so that a walk asks whether a key is in it without
//! searching. It is sorted only when its objects are read in the
//! collection's order: a count or a meeting with another set needs no order,
//! and sorting a set of thousands of keys took most of a count's time. Keys
//! come from the file, which is untrusted: a crafted file can make them
//! collide and slow a lookup down, but a collision is resolved by comparing
//! the keys, so it cannot change an answer.

/// Encoded primary keys, gathered, then looked up, and sorted when asked.
#[derive(Debug, Default)]
pub(crate) struct KeySet {
    /// The keys one after another.
    bytes: Vec<u8>,
    /// Where each key ends in `bytes`, in the order of the keys.
    ends: Vec<usize>,
    /// For each slot, one more than the position of the key there, or 0 for
    /// none: open addressing over `hash`, a power of two in size.
    table: Vec<u32>,
    /// Whether the keys are in ascending order.
    sorted: bool,
}

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

    /// Drops the repeats and makes the table, keeping the keys in the order
    /// they were gathered.
    pub(crate) fn seal(mut self) -> Self {
        let mut dropped: Option<Vec<bool>> = None;
        let len = self.len();

        self.sorted = (1..len).all(|at| self.get(at - 1) < self.get(at));
        // Keys in ascending order have no repeats to look for.
        self.table = self.table_of(!self.sorted, |at| {
            dropped.get_or_insert_with(|| vec![false; len])[at] = true;
        });

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
        if self.table.is_empty() {
            return false;
        }

        let mask = self.table.len() - 1;
        let mut slot = slot_of(key, mask);

        loop {
            match self.table[slot] {
                0 => return false,
                at if self.get(at as usize - 1) == key => return true,
                _ => slot = (slot + 1) & mask,
            }
        }
    }

    /// The table of the keys' hashes. With `repeats`, it calls `repeat` with
    /// the position of each key that one before it holds already, which it
    /// leaves out; without, the keys are known to be different, and a key in
    /// a slot is passed over without comparing.
    fn table_of(&self, repeats: bool, mut repeat: impl FnMut(usize)) -> Vec<u32> {
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
    /// the table again.
    fn arrange(&mut self, order: &[usize]) {
        let mut bytes = Vec::with_capacity(self.bytes.len());
        let mut ends = Vec::with_capacity(order.len());

        for &at in order {
            bytes.extend_from_slice(self.get(at));
            ends.push(bytes.len());
        }

        self.bytes = bytes;
        self.ends = ends;
        self.table = self.table_of(false, |_| {});
    }
}

#[cfg(test)]
mod tests {
    use super::KeySet;

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
}
