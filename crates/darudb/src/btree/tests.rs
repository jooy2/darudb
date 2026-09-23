//! The B+tree against a model: random changes, checked after every commit
//! against a `BTreeMap` and against the structural rules of the format.

use std::collections::{BTreeMap, HashSet};
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::format::{StoredValue, content_len, max_key_len};
use crate::storage::sim::SimDisk;
use crate::storage::{Cache, Pager};
use crate::testing::Rng;

const TREE: u64 = 16;

/// A write transaction reduced to what the B+tree needs, over a simulated
/// disk. Pages a commit releases are free again at the next one, as if no
/// reader were holding them.
struct Harness {
    loader: Loader,
    store: TestStore,
    root: Option<Child>,
}

struct TestStore {
    pager: Arc<Pager>,
    txn: u64,
    next: u64,
    free: Vec<u64>,
    fresh: HashSet<u64>,
    retired: Vec<u64>,
}

impl Store for TestStore {
    fn txn(&self) -> u64 {
        self.txn
    }

    fn allocate(&mut self) -> Result<u64> {
        let page = self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        });

        assert!(self.fresh.insert(page), "page {page} handed out twice");

        Ok(page)
    }

    fn allocate_run(&mut self, pages: u64) -> Result<u64> {
        let first = self.next;

        self.next += pages;
        self.fresh.extend(first..first + pages);

        Ok(first)
    }

    fn release(&mut self, page: u64) {
        if self.fresh.remove(&page) {
            self.free.push(page);
        } else {
            self.retired.push(page);
        }
    }

    fn write_run(&mut self, first: u64, bytes: &mut [u8]) -> Result<Vec<Check>> {
        self.pager.write_run(first, bytes)
    }

    fn run_pages(&self) -> usize {
        // Short runs, so that a value of a few pages takes several writes.
        2
    }
}

impl Harness {
    fn new(page_size: usize) -> Self {
        let disk: Arc<dyn crate::storage::FileIo> = Arc::new(SimDisk::default());
        let pager = Arc::new(Pager::new(disk, page_size, PathBuf::from("test.darudb")));

        Self {
            loader: Loader::new(Arc::clone(&pager), Arc::new(Cache::new(64))),
            store: TestStore {
                pager,
                txn: 1,
                next: 1,
                free: Vec::new(),
                fresh: HashSet::new(),
                retired: Vec::new(),
            },
            root: None,
        }
    }

    fn insert(&mut self, key: &[u8], value: &[u8]) -> bool {
        insert(
            &self.loader,
            &mut self.store,
            TREE,
            &mut self.root,
            key,
            value,
        )
        .unwrap()
    }

    fn remove(&mut self, key: &[u8]) -> bool {
        remove(&self.loader, &mut self.store, TREE, &mut self.root, key).unwrap()
    }

    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        get(&self.loader, TREE, self.root.as_ref(), key).unwrap()
    }

    fn entries(&self, start: Bound<&[u8]>, end: Bound<&[u8]>) -> Vec<(Vec<u8>, Vec<u8>)> {
        Range::new(&self.loader, TREE, self.root.as_ref(), start, end)
            .unwrap()
            .collect::<Result<_>>()
            .unwrap()
    }

    /// Writes every page the transaction changed and starts the next one.
    fn commit(&mut self) {
        let page_size = self.loader.page_size();
        let mut pages = Vec::new();

        if let Some(root) = self.root.take() {
            let pointer = finish(page_size, self.store.txn, TREE, root, &mut pages).unwrap();

            self.root = Some(Child::Clean(pointer));
        }

        for mut page in pages {
            let checks = self
                .store
                .pager
                .write_run(page.page, &mut page.bytes)
                .unwrap();

            assert_eq!(checks, [page.pointer.check]);
        }

        self.store.txn += 1;
        self.store.fresh.clear();
        self.store.free.append(&mut self.store.retired);
    }

    /// Walks the committed tree and checks every rule the format sets.
    /// Returns the number of entries.
    fn check_structure(&self) -> usize {
        let Some(root) = &self.root else {
            return 0;
        };
        let capacity = content_len(self.loader.page_size());
        let mut seen = HashSet::new();
        let mut count = 0;
        let mut pending = vec![(
            root.clone(),
            None::<u8>,
            None::<Vec<u8>>,
            None::<Vec<u8>>,
            true,
        )];

        while let Some((child, level, low, high, is_root)) = pending.pop() {
            if let Child::Clean(pointer) = &child {
                assert!(
                    seen.insert(pointer.page),
                    "page {} reached twice",
                    pointer.page
                );
            }

            let node = resolve(&self.loader, &child, TREE, level).unwrap();

            assert!(node.len() <= capacity, "a node larger than its page");

            let in_bounds = |key: &[u8]| {
                low.as_deref().is_none_or(|low| key >= low)
                    && high.as_deref().is_none_or(|high| key < high)
            };

            match &*node {
                Node::Leaf(entries) => {
                    assert!(
                        is_root || !entries.is_empty(),
                        "an empty leaf below the root"
                    );

                    for pair in entries.windows(2) {
                        assert!(pair[0].key < pair[1].key);
                    }

                    for entry in entries {
                        assert!(in_bounds(&entry.key), "a key outside its parent's range");
                    }

                    count += entries.len();
                }
                Node::Branch(branch) => {
                    assert!(!branch.keys.is_empty(), "a branch without keys");
                    assert_eq!(branch.children.len(), branch.keys.len() + 1);

                    for pair in branch.keys.windows(2) {
                        assert!(pair[0] < pair[1]);
                    }

                    for (index, child) in branch.children.iter().enumerate() {
                        let child_low = if index == 0 {
                            low.clone()
                        } else {
                            Some(branch.keys[index - 1].clone())
                        };
                        let child_high = branch.keys.get(index).cloned().or(high.clone());

                        pending.push((
                            child.clone(),
                            Some(branch.level - 1),
                            child_low,
                            child_high,
                            false,
                        ));
                    }
                }
            }
        }

        count
    }
}

fn value_of(rng: &mut Rng, page_size: usize) -> Vec<u8> {
    let len = match rng.below(10) {
        0 => rng.index(3 * content_len(page_size)),
        1 => 0,
        _ => rng.index(200),
    };

    rng.bytes(len)
}

fn key_of(rng: &mut Rng, page_size: usize) -> Vec<u8> {
    if rng.below(50) == 0 {
        let mut key = rng.bytes(max_key_len(page_size));

        key[0] = u8::try_from(rng.below(40)).unwrap();

        return key;
    }

    // A small alphabet, so that keys are inserted again and removed often.
    let len = 1 + rng.index(12);

    (0..len)
        .map(|_| b'a' + u8::try_from(rng.below(6)).unwrap())
        .collect()
}

#[test]
fn random_changes_match_a_model() {
    for seed in 0..12 {
        let page_size = if seed % 3 == 0 { 16384 } else { 4096 };
        let mut rng = Rng::new(seed);
        let mut harness = Harness::new(page_size);
        let mut model = BTreeMap::new();

        for round in 0..40 {
            for _ in 0..rng.index(120) {
                let key = key_of(&mut rng, page_size);

                if rng.below(3) == 0 {
                    assert_eq!(
                        harness.remove(&key),
                        model.remove(&key).is_some(),
                        "seed {seed}"
                    );
                } else {
                    let value = value_of(&mut rng, page_size);

                    assert_eq!(
                        harness.insert(&key, &value),
                        model.insert(key, value).is_some(),
                        "seed {seed}"
                    );
                }
            }

            if round % 2 == 0 {
                harness.commit();
            }

            let expected: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

            assert_eq!(
                harness.entries(Bound::Unbounded, Bound::Unbounded),
                expected,
                "seed {seed} round {round}"
            );

            if harness
                .root
                .as_ref()
                .is_some_and(|root| matches!(root, Child::Clean(_)))
            {
                assert_eq!(
                    harness.check_structure(),
                    model.len(),
                    "seed {seed} round {round}"
                );
            }
        }

        for key in model.keys().take(20) {
            assert_eq!(harness.get(key).as_ref(), model.get(key));
        }
    }
}

#[test]
fn removing_everything_empties_the_tree_and_frees_its_pages() {
    let mut rng = Rng::new(99);
    let mut harness = Harness::new(4096);
    let mut keys = Vec::new();

    for index in 0..2000u32 {
        let key = index.to_be_bytes().to_vec();

        harness.insert(&key, &rng.bytes(if index % 97 == 0 { 9000 } else { 40 }));
        keys.push(key);
    }

    harness.commit();

    assert!(harness.check_structure() == 2000);

    rng.shuffle(&mut keys);

    for key in &keys {
        assert!(harness.remove(key));
    }

    assert!(harness.root.is_none());

    harness.commit();

    // Every page ever handed out is free again: none leaked, none twice.
    let free: HashSet<u64> = harness.store.free.iter().copied().collect();

    assert_eq!(free.len(), harness.store.free.len(), "a page freed twice");
    assert_eq!(free, (1..harness.store.next).collect());
}

#[test]
fn ranges_respect_their_bounds() {
    let mut harness = Harness::new(4096);

    for index in 0..500u32 {
        harness.insert(&index.to_be_bytes(), b"v");
    }

    harness.commit();

    let keys = |start: Bound<&[u8]>, end: Bound<&[u8]>| -> Vec<u32> {
        harness
            .entries(start, end)
            .into_iter()
            .map(|(key, _)| u32::from_be_bytes(key.try_into().unwrap()))
            .collect()
    };
    let at = |index: u32| index.to_be_bytes();

    assert_eq!(
        keys(Bound::Included(&at(10)), Bound::Excluded(&at(13))),
        [10, 11, 12]
    );
    assert_eq!(
        keys(Bound::Excluded(&at(10)), Bound::Included(&at(13))),
        [11, 12, 13]
    );
    assert_eq!(
        keys(Bound::Included(&at(498)), Bound::Unbounded),
        [498, 499]
    );
    assert_eq!(keys(Bound::Unbounded, Bound::Excluded(&at(2))), [0, 1]);
    assert!(keys(Bound::Included(&at(600)), Bound::Unbounded).is_empty());
    assert_eq!(
        harness.entries(Bound::Unbounded, Bound::Unbounded).len(),
        500
    );
}

#[test]
fn a_committed_page_is_never_written_again() {
    let mut harness = Harness::new(4096);

    for index in 0..300u32 {
        harness.insert(&index.to_be_bytes(), &[1; 50]);
    }

    harness.commit();

    let committed = collect_pages(&harness);

    // Change one entry: only a path is copied, into new pages.
    harness.store.free.clear();
    harness.insert(&7u32.to_be_bytes(), &[2; 50]);

    let Some(Child::Dirty { page, .. }) = &harness.root else {
        panic!("the root was not copied");
    };

    assert!(!committed.contains(page));
    assert!(
        harness
            .store
            .retired
            .iter()
            .all(|page| committed.contains(page))
    );
}

fn collect_pages(harness: &Harness) -> HashSet<u64> {
    let mut pages = HashSet::new();
    let mut pending: Vec<(Child, Option<u8>)> =
        harness.root.iter().cloned().map(|r| (r, None)).collect();

    while let Some((child, level)) = pending.pop() {
        if let Child::Clean(pointer) = &child {
            pages.insert(pointer.page);
        }

        if let Node::Branch(branch) = &*resolve(&harness.loader, &child, TREE, level).unwrap() {
            pending.extend(
                branch
                    .children
                    .iter()
                    .cloned()
                    .map(|c| (c, Some(branch.level - 1))),
            );
        }
    }

    pages
}

#[test]
fn a_value_just_past_the_inline_limit_goes_to_an_overflow_run() {
    let mut harness = Harness::new(4096);
    let big = vec![7u8; 1000];

    harness.insert(b"k", &big);

    let Some(Child::Dirty { node, .. }) = &harness.root else {
        panic!();
    };
    let Node::Leaf(entries) = node.as_ref() else {
        panic!();
    };

    assert!(matches!(entries[0].value, StoredValue::Overflow(_)));
    assert_eq!(harness.get(b"k"), Some(big.clone()));

    harness.commit();

    assert_eq!(harness.get(b"k"), Some(big));
}
