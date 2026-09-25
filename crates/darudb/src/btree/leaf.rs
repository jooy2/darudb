//! A leaf a write transaction holds and changes, laid out as its page.
//!
//! The page's content is what a leaf page holds on disk (`format::node`): a
//! slot array after the header, free space, and the entries' cells at the
//! end, before the check. An entry is inserted by writing its cell below the
//! lowest one and a slot in the array, and removed by dropping its slot; the
//! bytes of a removed cell are counted and reclaimed by compacting the page
//! when an insert needs them, and before the page is written, so that no
//! removed value stays in a page on disk.
//!
//! A leaf kept as a vector of entries would cost an allocation for every key
//! and value, a walk over every entry to learn its size, and an encoding at
//! commit; here a search reads one buffer, the size is a sum kept as it
//! changes, and the page is written as it is.

use super::node::compare;
use crate::format::{
    CONTENT_OFFSET, LeafEntry, OverflowRef, StoredRef, StoredValue, cell_len, check_offset,
    encode_leaf, leaf_cell, leaf_entry, leaf_inline, leaf_key, leaf_value, set_slot, write_cell,
};

/// A leaf of a write transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Leaf {
    /// The whole page: its frame is filled in when it is written.
    page: Vec<u8>,
    count: usize,
    /// Where the lowest cell starts; free space ends there.
    low: usize,
    /// Bytes of cells no slot points to any more, among the others.
    garbage: usize,
}

impl Leaf {
    /// An empty leaf for a page of `page_size` bytes.
    pub(crate) fn new(page_size: usize) -> Self {
        Self {
            page: vec![0; page_size],
            count: 0,
            low: check_offset(page_size),
            garbage: 0,
        }
    }

    /// A copy of the leaf with `count` entries on `page`, which
    /// `check_leaf` has passed.
    #[cfg(test)]
    pub(crate) fn from_page(page: &[u8], count: usize) -> Self {
        let (size, low) = crate::format::leaf_extent(page, count);

        Self::from_loaded(page, count, size, low)
    }

    /// [`from_page`](Self::from_page) for a page whose entries take `size`
    /// bytes, slots included, and whose lowest cell starts at `low`, as the
    /// cached node knows them: copying a leaf for a write transaction to
    /// change then reads no cell of it.
    pub(crate) fn from_loaded(page: &[u8], count: usize, size: usize, low: usize) -> Self {
        let end = check_offset(page.len());
        let cells = size.saturating_sub(2 * count);

        Self {
            page: page.to_vec(),
            count,
            low,
            // Cells that overlap, which only a damaged page has, count as
            // none: the leaf then splits sooner than it has to, no more.
            garbage: end.saturating_sub(low).saturating_sub(cells),
        }
    }

    /// A leaf holding `entries`, in order, which fit in a page.
    pub(crate) fn from_entries(page_size: usize, entries: &[LeafEntry]) -> Self {
        let mut page = vec![0; page_size];

        encode_leaf(entries, &mut page);

        let used: usize = entries.iter().map(|entry| entry.len() - 2).sum();

        Self {
            page,
            count: entries.len(),
            low: check_offset(page_size) - used,
            garbage: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.count
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The bytes of a page's content the entries take, slots included.
    pub(crate) fn size(&self) -> usize {
        2 * self.count + check_offset(self.page.len()) - self.low - self.garbage
    }

    #[cfg(test)]
    pub(crate) fn page(&self) -> &[u8] {
        &self.page
    }

    pub(crate) fn key(&self, index: usize) -> &[u8] {
        leaf_key(&self.page, index)
    }

    pub(crate) fn value(&self, index: usize) -> Result<StoredRef<'_>, &'static str> {
        leaf_value(&self.page, index)
    }

    pub(crate) fn entry(&self, index: usize) -> Result<(&[u8], StoredRef<'_>), &'static str> {
        leaf_entry(&self.page, index)
    }

    pub(crate) fn inline(&self, index: usize) -> Option<(&[u8], &[u8])> {
        leaf_inline(&self.page, index)
    }

    /// Where `key` is, or where it would go.
    pub(crate) fn search(&self, key: &[u8]) -> Result<usize, usize> {
        let (mut low, mut high) = (0, self.count);

        while low < high {
            let middle = low + (high - low) / 2;

            match compare(self.key(middle), key) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Ok(middle),
            }
        }

        Err(low)
    }

    /// Inserts `key` and `value` as entry `index`, if they fit in the page;
    /// returns whether they did. The page is compacted first when only the
    /// bytes of removed cells make room.
    pub(crate) fn insert(&mut self, index: usize, key: &[u8], value: StoredRef<'_>) -> bool {
        let len = cell_len(key.len(), value);

        if len + 2 > self.free() {
            if len + 2 > self.free() + self.garbage {
                return false;
            }

            self.compact();
        }

        let slots = CONTENT_OFFSET + 2 * index;
        let slots_end = CONTENT_OFFSET + 2 * self.count;

        self.page.copy_within(slots..slots_end, slots + 2);
        self.low -= len;
        write_cell(&mut self.page, self.low, key, value);
        set_slot(&mut self.page, index, self.low);
        self.count += 1;

        true
    }

    /// Replaces the value of entry `index`, whose key is `key`, with `value`
    /// in the entry's own cell, when the new cell is no longer than the old
    /// one. Returns `None`, having changed nothing, when it is longer, and
    /// otherwise the overflow run of the old value, if it had one, for the
    /// caller to give back.
    ///
    /// A leaf filled in key order has no room left, so replacing a value by
    /// a removal and an insert compacted the whole page every time; a value
    /// that keeps its length, as most replacements do, changes in place. The
    /// old cell's bytes past the new one's end count as removed, and go when
    /// the page is compacted.
    pub(crate) fn overwrite(
        &mut self,
        index: usize,
        key: &[u8],
        value: StoredRef<'_>,
    ) -> Result<Option<Option<OverflowRef>>, &'static str> {
        let (at, len) = leaf_cell(&self.page, index);
        let new_len = cell_len(key.len(), value);

        if new_len > len {
            return Ok(None);
        }

        let run = match self.value(index)? {
            StoredRef::Inline(_) => None,
            StoredRef::Overflow(reference) => Some(reference),
        };

        write_cell(&mut self.page, at, key, value);
        self.garbage += len - new_len;

        Ok(Some(run))
    }

    /// The bytes entry `index` takes, its slot included.
    pub(crate) fn entry_size(&self, index: usize) -> usize {
        leaf_cell(&self.page, index).1 + 2
    }

    /// Moves the entries from `at` on into a new leaf, which it returns.
    pub(crate) fn split_off(&mut self, at: usize) -> Leaf {
        let mut right = Leaf::new(self.page.len());

        for index in at..self.count {
            let (cell, len) = leaf_cell(&self.page, index);

            right.low -= len;
            right.page[right.low..right.low + len].copy_from_slice(&self.page[cell..cell + len]);
            set_slot(&mut right.page, index - at, right.low);
            self.garbage += len;
        }

        right.count = self.count - at;
        self.page[CONTENT_OFFSET + 2 * at..CONTENT_OFFSET + 2 * self.count].fill(0);
        self.count = at;

        right
    }

    /// Removes entry `index`, and returns the overflow run of its value, if
    /// it had one, for the caller to give back.
    pub(crate) fn remove(&mut self, index: usize) -> Result<Option<OverflowRef>, &'static str> {
        let run = match self.value(index)? {
            StoredRef::Inline(_) => None,
            StoredRef::Overflow(reference) => Some(reference),
        };
        let (_, len) = leaf_cell(&self.page, index);
        let slots_end = CONTENT_OFFSET + 2 * self.count;

        self.page.copy_within(
            CONTENT_OFFSET + 2 * (index + 1)..slots_end,
            CONTENT_OFFSET + 2 * index,
        );
        self.page[slots_end - 2..slots_end].fill(0);
        self.count -= 1;
        self.garbage += len;

        Ok(run)
    }

    /// The entries, decoded: for a split or a merge, which are rare.
    pub(crate) fn to_entries(&self) -> Result<Vec<LeafEntry>, &'static str> {
        (0..self.count)
            .map(|index| {
                let (key, value) = self.entry(index)?;

                Ok(LeafEntry {
                    key: key.to_vec(),
                    value: match value {
                        StoredRef::Inline(value) => StoredValue::Inline(value.to_vec()),
                        StoredRef::Overflow(reference) => StoredValue::Overflow(reference),
                    },
                })
            })
            .collect()
    }

    /// The page, compacted if any cell was removed, with its frame zeroed
    /// for the header and the check to be written.
    pub(crate) fn into_page(mut self) -> Vec<u8> {
        if self.garbage > 0 {
            self.compact();
        }

        let end = check_offset(self.page.len());

        self.page[..CONTENT_OFFSET].fill(0);
        self.page[end..].fill(0);
        self.page
    }

    /// The contiguous bytes between the slots and the lowest cell.
    fn free(&self) -> usize {
        self.low - (CONTENT_OFFSET + 2 * self.count)
    }

    /// Rewrites the cells one after another from the end of the page, in slot
    /// order, leaving zeros where removed cells were.
    fn compact(&mut self) {
        let mut page = vec![0; self.page.len()];
        let mut cursor = check_offset(page.len());

        for index in 0..self.count {
            let (at, len) = leaf_cell(&self.page, index);

            cursor -= len;
            page[cursor..cursor + len].copy_from_slice(&self.page[at..at + len]);
            set_slot(&mut page, index, cursor);
        }

        page[..CONTENT_OFFSET].copy_from_slice(&self.page[..CONTENT_OFFSET]);
        self.page = page;
        self.low = cursor;
        self.garbage = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Check, check_leaf, decode_leaf};
    use crate::testing::Rng;

    const PAGE: usize = 4096;

    fn value_of(rng: &mut Rng) -> StoredValue {
        if rng.below(8) == 0 {
            StoredValue::Overflow(OverflowRef {
                first: 1 + rng.below(1000),
                txn: 1 + rng.below(1000),
                pages: 1,
                len: 1 + rng.below(4000),
                check: Check::of(&[b"run"]),
            })
        } else {
            StoredValue::Inline(
                (0..rng.below(200))
                    .map(|_| u8::try_from(rng.below(256)).unwrap())
                    .collect(),
            )
        }
    }

    /// Random inserts, replacements and removals, compared with a sorted
    /// vector of entries, the page checked and decoded after every change.
    #[test]
    fn a_leaf_changes_as_a_sorted_list_of_entries_does() {
        let mut rng = Rng::new(21);

        for _ in 0..40 {
            let mut leaf = Leaf::new(PAGE);
            let mut model: Vec<LeafEntry> = Vec::new();

            for _ in 0..300 {
                let key: Vec<u8> = (0..1 + rng.below(12))
                    .map(|_| b"abcd"[rng.index(4)])
                    .collect();
                let found = leaf.search(&key);

                assert_eq!(
                    found,
                    model.binary_search_by(|entry| entry.key.cmp(&key)),
                    "search for {key:?}"
                );

                if rng.below(3) == 0 {
                    if let Ok(index) = found {
                        let run = leaf.remove(index).unwrap();
                        let removed = model.remove(index);

                        assert_eq!(
                            run,
                            match removed.value {
                                StoredValue::Overflow(reference) => Some(reference),
                                StoredValue::Inline(_) => None,
                            }
                        );
                    }
                } else {
                    let value = value_of(&mut rng);
                    // Half the replacements try the entry's own cell first.
                    let overwritten = match found {
                        Ok(index) if rng.below(2) == 0 => leaf
                            .overwrite(index, &key, value.as_stored())
                            .unwrap()
                            .map(|run| (index, run)),
                        _ => None,
                    };

                    if let Some((index, run)) = overwritten {
                        let old = std::mem::replace(&mut model[index].value, value);

                        assert_eq!(
                            run,
                            match old {
                                StoredValue::Overflow(reference) => Some(reference),
                                StoredValue::Inline(_) => None,
                            }
                        );
                    } else {
                        let at = match found {
                            Ok(index) => {
                                leaf.remove(index).unwrap();
                                model.remove(index);
                                index
                            }
                            Err(index) => index,
                        };
                        let entry = LeafEntry { key, value };

                        if leaf.insert(at, &entry.key, entry.value.as_stored()) {
                            model.insert(at, entry);
                        }
                    }
                }

                let size: usize = model.iter().map(LeafEntry::len).sum();

                assert_eq!(leaf.size(), size);
                assert!(size <= PAGE - CONTENT_OFFSET - 16);
                assert_eq!(leaf.to_entries().unwrap(), model);

                // A page on disk holds at least one entry.
                if !model.is_empty() {
                    check_leaf(leaf.page(), leaf.len()).unwrap();
                    assert_eq!(decode_leaf(leaf.page(), leaf.len()).unwrap(), model);
                }

                let again = Leaf::from_page(leaf.page(), leaf.len());

                assert_eq!(
                    again.size(),
                    size,
                    "a copy of the page counts the same size"
                );
            }

            if model.is_empty() {
                continue;
            }

            let written = leaf.clone().into_page();

            check_leaf(&written, model.len()).unwrap();
            assert_eq!(decode_leaf(&written, model.len()).unwrap(), model);
            assert_eq!(
                decode_leaf(&Leaf::from_entries(PAGE, &model).into_page(), model.len()).unwrap(),
                model
            );
        }
    }

    #[test]
    fn a_split_leaf_keeps_its_entries_in_two_pages() {
        let mut rng = Rng::new(5);

        for _ in 0..100 {
            let mut leaf = Leaf::new(PAGE);
            let mut model: Vec<LeafEntry> = Vec::new();

            for _ in 0..60 {
                let key: Vec<u8> = (0..1 + rng.below(8))
                    .map(|_| b"abcd"[rng.index(4)])
                    .collect();

                if let Err(at) = leaf.search(&key) {
                    let entry = LeafEntry {
                        key,
                        value: value_of(&mut rng),
                    };

                    if leaf.insert(at, &entry.key, entry.value.as_stored()) {
                        model.insert(at, entry);
                    }
                }
            }

            let at = rng.index(model.len() + 1);
            let right = leaf.split_off(at);
            let (left_model, right_model) = model.split_at(at);

            assert_eq!(leaf.to_entries().unwrap(), left_model);
            assert_eq!(right.to_entries().unwrap(), right_model);
            assert_eq!(
                leaf.size(),
                left_model.iter().map(LeafEntry::len).sum::<usize>()
            );
            assert_eq!(
                right.size(),
                right_model.iter().map(LeafEntry::len).sum::<usize>()
            );

            // The left page reclaims what moved out when it next needs room.
            let mut left_model = left_model.to_vec();

            // A key above every other keeps the leaf's keys in order.
            if leaf.insert(left_model.len(), b"\xff\xff", StoredRef::Inline(&[1; 100])) {
                left_model.push(LeafEntry {
                    key: b"\xff\xff".to_vec(),
                    value: StoredValue::Inline(vec![1; 100]),
                });
            }

            assert_eq!(leaf.to_entries().unwrap(), left_model);

            if !left_model.is_empty() {
                check_leaf(&leaf.clone().into_page(), left_model.len()).unwrap();
            }
        }
    }

    #[test]
    fn a_removed_value_is_not_in_the_page_written() {
        let secret = [0xAB; 64];
        let mut leaf = Leaf::new(PAGE);

        assert!(leaf.insert(0, b"a", StoredRef::Inline(b"kept")));
        assert!(leaf.insert(1, b"b", StoredRef::Inline(&secret)));
        assert!(leaf.insert(2, b"c", StoredRef::Inline(b"kept too")));
        leaf.remove(1).unwrap();

        let page = leaf.into_page();

        assert!(!page.windows(8).any(|window| window == [0xAB; 8]));
        assert_eq!(
            decode_leaf(&page, 2).unwrap(),
            [
                LeafEntry {
                    key: b"a".to_vec(),
                    value: StoredValue::Inline(b"kept".to_vec()),
                },
                LeafEntry {
                    key: b"c".to_vec(),
                    value: StoredValue::Inline(b"kept too".to_vec()),
                },
            ]
        );
    }

    #[test]
    fn an_insert_that_does_not_fit_changes_nothing() {
        let mut leaf = Leaf::new(PAGE);
        let big = vec![7u8; 900];
        let mut added = 0;

        while leaf.insert(
            added,
            &[u8::try_from(added).unwrap()],
            StoredRef::Inline(&big),
        ) {
            added += 1;
        }

        let before = leaf.clone();

        assert!(!leaf.insert(0, b"", StoredRef::Inline(&big)));
        assert_eq!(leaf, before);
    }
}
