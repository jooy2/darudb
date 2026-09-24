//! The page cache, keyed by page number and check.
//!
//! A reader always knows the check it expects, from the pointer that led it to
//! the page, so a cached copy of an older page at the same number never
//! matches. Nothing is invalidated when a commit is published: entries that no
//! longer match anything simply age out.

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

use crate::format::Check;

/// A bounded cache of decoded pages, evicting the oldest insertion first.
#[derive(Debug)]
pub(crate) struct Cache<V> {
    inner: Mutex<Inner<V>>,
    capacity: usize,
}

#[derive(Debug)]
struct Inner<V> {
    entries: HashMap<(u64, Check), Arc<V>, BuildHasherDefault<Fold>>,
    order: VecDeque<(u64, Check)>,
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

impl<V> Cache<V> {
    /// A cache that holds up to `capacity` pages.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::default(),
                order: VecDeque::new(),
            }),
            capacity,
        }
    }

    /// The cached copy of page `page` with check `check`, if there is one.
    pub(crate) fn get(&self, page: u64, check: &Check) -> Option<Arc<V>> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        inner.entries.get(&(page, *check)).cloned()
    }

    /// Remembers `value` as page `page` with check `check`.
    pub(crate) fn insert(&self, page: u64, check: Check, value: Arc<V>) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        if inner.entries.insert((page, check), value).is_none() {
            inner.order.push_back((page, check));
        }

        while inner.entries.len() > self.capacity {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };

            inner.entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_found_only_under_its_own_check() {
        let cache = Cache::new(4);
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
    fn the_oldest_entry_goes_first() {
        let cache = Cache::new(2);
        let check = Check::ZERO;

        cache.insert(1, check, Arc::new(1));
        cache.insert(2, check, Arc::new(2));
        cache.insert(3, check, Arc::new(3));

        assert!(cache.get(1, &check).is_none());
        assert!(cache.get(2, &check).is_some());
        assert!(cache.get(3, &check).is_some());
    }
}
