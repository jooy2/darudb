//! Helpers shared by the engine's own tests.

/// A small deterministic random number generator (SplitMix64), so that a
/// failing randomized test can be replayed from its seed.
#[derive(Debug, Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);

        let mut z = self.0;

        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);

        z ^ (z >> 31)
    }

    /// A number from 0 up to, but not including, `bound`.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    /// An index into a collection of `len` items.
    pub(crate) fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap()
    }

    /// `len` random bytes.
    pub(crate) fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u64().to_le_bytes()[0]).collect()
    }

    /// Puts `items` in a random order.
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let other = self.index(index + 1);

            items.swap(index, other);
        }
    }
}
