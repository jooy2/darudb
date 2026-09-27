//! The page cache, keyed by page number and check.
//!
//! A reader always knows the check it expects, from the pointer that led it to
//! the page, so a cached copy of an older page at the same number never
//! matches. Nothing is invalidated when a commit is published: entries that no
//! longer match anything simply age out.
//!
//! The cache counts the bytes its entries hold, as [`Weigh`] reports them,
//! rather than the entries themselves, since a node keeps more than its page.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::format::Check;

/// A bounded cache of decoded pages, evicting the oldest insertion first.
#[derive(Debug)]
pub(crate) struct Cache<V> {
    inner: Mutex<Inner<V>>,
    /// The bytes the entries may hold.
    capacity: usize,
    /// The entries the cache keeps whatever they hold.
    least: usize,
}

#[derive(Debug)]
struct Inner<V> {
    entries: HashMap<(u64, Check), Arc<V>, BuildHasherDefault<Fold>>,
    /// The keys in the order they were inserted, oldest first. A key taken
    /// out stays here until it is evicted in its turn or the order is
    /// compacted, and one inserted again comes here twice.
    order: VecDeque<(u64, Check)>,
    /// The bytes the entries hold.
    used: usize,
}

/// What a cached value holds, in bytes.
pub(crate) trait Weigh {
    fn weight(&self) -> usize;
}

/// The hash of a cache key: its words folded together.
///
/// The check in every key is already a hash of the page, so a hash built to
/// resist chosen keys would spend its time for nothing on every lookup. A
/// file crafted to make keys collide can slow the cache down, but not by
/// more than the few thousand entries it holds.
#[derive(Debug, Default)]
struct Fold(u64);

impl Fold {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517C_C1B7_2722_0A95);
    }
}

impl Hasher for Fold {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];

            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.add(word);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl<V: Weigh> Cache<V> {
    /// A cache whose entries hold up to `capacity` bytes, which keeps its
    /// newest `least` entries whatever they hold.
    pub(crate) fn new(capacity: usize, least: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::default(),
                order: VecDeque::new(),
                used: 0,
            }),
            capacity,
            least,
        }
    }

    /// How many bytes the entries may hold.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many entries the cache holds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// How many bytes the entries hold.
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.lock().used
    }

    fn lock(&self) -> MutexGuard<'_, Inner<V>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cached copy of page `page` with check `check`, if there is one.
    pub(crate) fn get(&self, page: u64, check: &Check) -> Option<Arc<V>> {
        self.lock().entries.get(&(page, *check)).cloned()
    }

    /// Takes the copy of page `page` with check `check` out of the cache, if
    /// there is one, for a caller that means to change it: one that nothing
    /// else holds is then the caller's to change, with no copy made.
    pub(crate) fn take(&self, page: u64, check: &Check) -> Option<Arc<V>> {
        let mut inner = self.lock();
        let taken = inner.entries.remove(&(page, *check))?;

        inner.used -= taken.weight();

        Some(taken)
    }

    /// Remembers `value` as page `page` with check `check`.
    pub(crate) fn insert(&self, page: u64, check: Check, value: Arc<V>) {
        let mut inner = self.lock();

        inner.used += value.weight();

        match inner.entries.insert((page, check), value) {
            Some(old) => inner.used -= old.weight(),
            None => inner.order.push_back((page, check)),
        }

        // Keys taken out pile up in the order of a cache that is not full,
        // where nothing is evicted: once they are as many as the entries,
        // they go, in one pass that keeps each key present at its newest
        // place.
        if inner.order.len() > 2 * inner.entries.len() + 64 {
            let Inner { entries, order, .. } = &mut *inner;
            let mut seen = HashSet::with_capacity(entries.len());
            let mut kept: Vec<(u64, Check)> = order
                .iter()
                .rev()
                .filter(|key| entries.contains_key(key) && seen.insert(**key))
                .copied()
                .collect();

            kept.reverse();
            *order = kept.into();
        }

        while inner.used > self.capacity && inner.entries.len() > self.least {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };

            if let Some(old) = inner.entries.remove(&oldest) {
                inner.used -= old.weight();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Weigh for &str {
        fn weight(&self) -> usize {
            self.len()
        }
    }

    #[test]
    fn a_page_is_found_only_under_its_own_check() {
        let cache = Cache::new(8, 1);
        let old = Check::of(&[b"old"]);
        let new = Check::of(&[b"new"]);

        cache.insert(7, old, Arc::new("old"));

        assert_eq!(cache.get(7, &old).as_deref(), Some(&"old"));
        assert!(
            cache.get(7, &new).is_none(),
            "a newer page at the same number"
        );
    }

    #[test]
    fn the_oldest_entries_go_first_until_the_rest_fit() {
        let cache = Cache::new(6, 1);
        let check = Check::ZERO;

        cache.insert(1, check, Arc::new("ab"));
        cache.insert(2, check, Arc::new("cd"));
        cache.insert(3, check, Arc::new("ef"));

        assert!(cache.get(1, &check).is_some(), "six bytes fit");

        cache.insert(4, check, Arc::new("ghij"));

        assert!(cache.get(1, &check).is_none());
        assert!(cache.get(2, &check).is_none());
        assert!(cache.get(3, &check).is_some());
        assert!(cache.get(4, &check).is_some());

        // An entry again under its key counts once, at its new weight.
        cache.insert(3, check, Arc::new("e"));
        cache.insert(5, check, Arc::new("k"));

        assert!(cache.get(3, &check).is_some());
        assert!(cache.get(4, &check).is_some());
        assert!(cache.get(5, &check).is_some());
    }

    /// An entry taken out is gone, and its bytes with it; the keys taken out
    /// do not pile up in the order of a cache that never fills.
    #[test]
    fn an_entry_taken_out_is_gone_and_counts_nothing() {
        let cache = Cache::new(1 << 20, 1);
        let check = Check::ZERO;

        cache.insert(1, check, Arc::new("abc"));

        assert_eq!(cache.take(1, &check).as_deref(), Some(&"abc"));
        assert!(cache.take(1, &check).is_none());
        assert!(cache.get(1, &check).is_none());
        assert_eq!(cache.used(), 0);

        for round in 0..10_000u64 {
            cache.insert(round % 7, check, Arc::new("x"));
            cache.take(round % 7, &check);
        }

        cache.insert(3, check, Arc::new("kept"));

        // Twice the one entry, the slack, and the key just inserted.
        assert!(cache.lock().order.len() <= 2 + 64 + 1);
        assert_eq!(cache.get(3, &check).as_deref(), Some(&"kept"));
    }

    #[test]
    fn the_least_entries_stay_whatever_they_hold() {
        let cache = Cache::new(1, 2);
        let check = Check::ZERO;

        cache.insert(1, check, Arc::new("abc"));
        cache.insert(2, check, Arc::new("def"));
        cache.insert(3, check, Arc::new("ghi"));

        assert!(cache.get(1, &check).is_none());
        assert!(cache.get(2, &check).is_some());
        assert!(cache.get(3, &check).is_some());
    }
}
