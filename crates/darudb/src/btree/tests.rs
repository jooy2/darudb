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
    /// Pages written after this commit are young; 0, as in a window no
    /// barrier has closed, unless a test says otherwise.
    young_after: u64,
}

impl Store for TestStore {
    fn txn(&self) -> u64 {
        self.txn
    }

    fn young_after(&self) -> u64 {
        self.young_after
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

    fn release(&mut self, page: u64, _written: u64) {
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
        let pager = Arc::new(Pager::new(
            disk,
            page_size,
            PathBuf::from("test.darudb"),
            None,
        ));

        Self {
            // About 64 pages, so that the tests evict and read pages again.
            loader: Loader::new(Arc::clone(&pager), Arc::new(Cache::new(64 * page_size, 16))),
            store: TestStore {
                pager,
                txn: 1,
                next: 1,
                free: Vec::new(),
                fresh: HashSet::new(),
                retired: Vec::new(),
                young_after: 0,
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
            true,
        )
        .unwrap()
            == Inserted::Replaced
    }

    fn remove(&mut self, key: &[u8]) -> bool {
        remove(&self.loader, &mut self.store, TREE, &mut self.root, key).unwrap()
    }

    fn remove_present(&mut self, key: &[u8]) -> bool {
        super::remove_present(&self.loader, &mut self.store, TREE, &mut self.root, key).unwrap()
    }

    /// Stores `value` under `key` through [`insert_with`], and returns the
    /// value it gave the visitor.
    fn insert_with(&mut self, key: &[u8], value: &[u8]) -> Option<Vec<u8>> {
        let mut visited = None;
        let inserted = super::insert_with(
            &self.loader,
            &mut self.store,
            TREE,
            &mut self.root,
            key,
            value,
            &mut |old| {
                visited = Some(old.to_vec());

                Ok(())
            },
        )
        .unwrap();

        assert_eq!(inserted == Inserted::Replaced, visited.is_some());
        visited
    }

    /// Tries to store `value` under `key` through [`insert_with`] with a
    /// visitor that refuses the value there, and returns whether there was
    /// one to refuse.
    fn refuse_insert(&mut self, key: &[u8], value: &[u8]) -> bool {
        let mut seen = false;
        let result = super::insert_with(
            &self.loader,
            &mut self.store,
            TREE,
            &mut self.root,
            key,
            value,
            &mut |_| {
                seen = true;

                Err(crate::error::Error::InvalidArgument {
                    message: "refused".to_owned(),
                })
            },
        );

        assert_eq!(result.is_err(), seen);
        seen
    }

    /// Removes `key` through [`remove_with`], and returns the value it gave
    /// the visitor.
    fn remove_with(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        let mut visited = None;
        let removed = super::remove_with(
            &self.loader,
            &mut self.store,
            TREE,
            &mut self.root,
            key,
            &mut |value| {
                visited = Some(value.to_vec());

                Ok(())
            },
        )
        .unwrap();

        assert_eq!(removed, visited.is_some());
        visited
    }

    /// Tries to remove `key` through [`remove_with`] with a visitor that
    /// refuses it, and returns whether there was a value to refuse.
    fn refuse_removal(&mut self, key: &[u8]) -> bool {
        let mut seen = false;
        let result = super::remove_with(
            &self.loader,
            &mut self.store,
            TREE,
            &mut self.root,
            key,
            &mut |_| {
                seen = true;

                Err(crate::error::Error::InvalidArgument {
                    message: "refused".to_owned(),
                })
            },
        );

        assert_eq!(result.is_err(), seen);
        seen
    }

    /// Changes the value under `key` through [`update_with`] to `value`, or
    /// keeps it with `None`, or refuses it with `refuse`, and returns whether
    /// there was one and the value it gave the change.
    fn update(&mut self, key: &[u8], value: Option<&[u8]>, refuse: bool) -> Option<Vec<u8>> {
        let mut visited = None;
        let result = super::update_with(
            &self.loader,
            &mut self.store,
            TREE,
            &mut self.root,
            key,
            &mut |old| {
                visited = Some(old.to_vec());

                if refuse {
                    return Err(crate::error::Error::InvalidArgument {
                        message: "refused".to_owned(),
                    });
                }

                Ok(value.map(<[u8]>::to_vec))
            },
        );

        match result {
            Ok(found) => assert_eq!(found, visited.is_some()),
            Err(_) => assert!(refuse && visited.is_some()),
        }

        visited
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

    fn entries_backward(&self, start: Bound<&[u8]>, end: Bound<&[u8]>) -> Vec<(Vec<u8>, Vec<u8>)> {
        Range::new_backward(&self.loader, TREE, self.root.as_ref(), start, end)
            .unwrap()
            .collect::<Result<_>>()
            .unwrap()
    }

    fn lent(
        &self,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        let range = if backward {
            Range::new_backward(&self.loader, TREE, self.root.as_ref(), start, end)
        } else {
            Range::new(&self.loader, TREE, self.root.as_ref(), start, end)
        };
        let mut entries = Vec::new();

        range
            .unwrap()
            .for_each(&mut |key, value| {
                entries.push((key.to_vec(), value.to_vec()));
                Ok(false)
            })
            .unwrap();
        entries
    }

    fn count(&self, start: Bound<&[u8]>, end: Bound<&[u8]>, backward: bool) -> u64 {
        let range = if backward {
            Range::new_backward(&self.loader, TREE, self.root.as_ref(), start, end)
        } else {
            Range::new(&self.loader, TREE, self.root.as_ref(), start, end)
        };

        range.unwrap().count_entries().unwrap()
    }

    /// Writes every page the transaction changed and starts the next one.
    fn commit(&mut self) {
        let mut pages = Vec::new();

        if let Some(root) = self.root.take() {
            let pointer =
                finish(&self.store.pager, self.store.txn, TREE, root, &mut pages).unwrap();

            self.root = Some(Child::Clean(pointer));
        }

        for page in pages {
            self.store
                .pager
                .write_sealed_run(page.page, page.bytes())
                .unwrap();
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

            assert!(node.size() <= capacity, "a node larger than its page");

            let in_bounds = |key: &[u8]| {
                low.as_deref().is_none_or(|low| key >= low)
                    && high.as_deref().is_none_or(|high| key < high)
            };

            match &node.to_node() {
                Node::Leaf(leaf) => {
                    let entries = leaf.to_entries().unwrap();

                    assert!(
                        is_root || !entries.is_empty(),
                        "an empty leaf below the root"
                    );

                    for pair in entries.windows(2) {
                        assert!(pair[0].key < pair[1].key);
                    }

                    for entry in &entries {
                        assert!(in_bounds(&entry.key), "a key outside its parent's range");
                    }

                    count += entries.len();
                }
                Node::Branch(branch) => {
                    assert!(!branch.keys.is_empty(), "a branch without keys");
                    assert_eq!(branch.children.len(), branch.keys.len() + 1);

                    for index in 1..branch.keys.len() {
                        assert!(branch.keys.get(index - 1) < branch.keys.get(index));
                    }

                    for (index, child) in branch.children.iter().enumerate() {
                        let child_low = if index == 0 {
                            low.clone()
                        } else {
                            Some(branch.keys.get(index - 1).to_vec())
                        };
                        let child_high = (index < branch.keys.len())
                            .then(|| branch.keys.get(index).to_vec())
                            .or(high.clone());

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
                    // Some removals go straight to the key, which takes
                    // nothing away when it is not there; some read its value
                    // on the way, and some of those are refused.
                    let removed = match rng.below(4) {
                        0 => harness.remove(&key),
                        1 => harness.remove_present(&key),
                        2 => {
                            let visited = harness.remove_with(&key);

                            assert_eq!(visited.as_ref(), model.get(&key), "seed {seed}");
                            visited.is_some()
                        }
                        _ => {
                            let refused = harness.refuse_removal(&key);

                            assert_eq!(refused, model.contains_key(&key), "seed {seed}");

                            continue;
                        }
                    };

                    assert_eq!(removed, model.remove(&key).is_some(), "seed {seed}");
                } else {
                    let value = value_of(&mut rng, page_size);

                    // Some inserts read the value they replace, and some of
                    // those are refused when there is one. Some updates
                    // replace the value, some keep it, and some are refused.
                    match rng.below(6) {
                        4 => {
                            let visited = harness.update(&key, Some(&value), false);

                            assert_eq!(visited.as_ref(), model.get(&key), "seed {seed}");

                            if visited.is_some() {
                                model.insert(key, value);
                            }
                        }
                        5 => {
                            let refuse = rng.below(2) == 0;
                            let visited = harness.update(&key, None, refuse);

                            assert_eq!(visited.as_ref(), model.get(&key), "seed {seed}");
                        }
                        0 => {
                            let visited = harness.insert_with(&key, &value);

                            assert_eq!(visited.as_ref(), model.get(&key), "seed {seed}");
                            model.insert(key, value);
                        }
                        1 => {
                            if !harness.refuse_insert(&key, &value) {
                                model.insert(key, value);
                            }
                        }
                        _ => assert_eq!(
                            harness.insert(&key, &value),
                            model.insert(key, value).is_some(),
                            "seed {seed}"
                        ),
                    }
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

/// A seeker finds what a lookup from the root finds, whatever order the keys
/// come in: ascending, descending, at random, each twice, and keys the tree
/// does not hold, in a tree changed and not committed yet and in one
/// committed, three levels deep and more.
#[test]
fn a_seeker_finds_what_a_lookup_from_the_root_finds() {
    let mut deepest = 0;

    for seed in 0..6 {
        let page_size = if seed % 3 == 0 { 16384 } else { 4096 };
        let mut rng = Rng::new(700 + seed);
        let mut harness = Harness::new(page_size);
        let mut model = BTreeMap::new();

        for round in 0..6 {
            for _ in 0..400 + rng.index(800) {
                let key = key_of(&mut rng, page_size);

                if rng.below(5) == 0 {
                    harness.remove(&key);
                    model.remove(&key);
                } else {
                    let value = value_of(&mut rng, page_size);

                    harness.insert(&key, &value);
                    model.insert(key, value);
                }
            }

            if round % 2 == 1 {
                harness.commit();
            }

            let mut probes: Vec<Vec<u8>> = model.keys().cloned().collect();

            probes.extend((0..300).map(|_| key_of(&mut rng, page_size)));

            for order in 0..4 {
                let mut keys = probes.clone();

                match order {
                    0 => keys.sort(),
                    1 => {
                        keys.sort();
                        keys.reverse();
                    }
                    2 => rng.shuffle(&mut keys),
                    _ => {
                        keys.extend(probes.iter().cloned());
                        rng.shuffle(&mut keys);
                    }
                }

                let mut seeker = Seeker::new(&harness.loader, TREE, harness.root.as_ref()).unwrap();

                for key in &keys {
                    let mut found = None;
                    let hit = seeker
                        .get_with(key, &mut |value| {
                            found = Some(value.to_vec());

                            Ok(())
                        })
                        .unwrap();

                    assert_eq!(hit, found.is_some());
                    assert_eq!(
                        found.as_ref(),
                        model.get(key),
                        "seed {seed} round {round} order {order}"
                    );
                    deepest = deepest.max(seeker.depth());
                }
            }
        }
    }

    assert!(
        deepest >= 2,
        "the trees reached {deepest} levels of branches"
    );
}

/// A removal that reads the value it takes out searches a committed node
/// before copying it, so a key that is not there copies nothing, whether
/// no node on the way was copied yet or only the upper ones were.
#[test]
fn a_removal_that_reads_its_value_copies_nothing_for_a_missing_key() {
    let mut harness = Harness::new(4096);

    for n in 0..2000u32 {
        harness.insert(&(n * 2).to_be_bytes(), &[7; 40]);
    }

    harness.commit();

    assert_eq!(harness.remove_with(&5u32.to_be_bytes()), None);
    assert!(matches!(harness.root, Some(Child::Clean(_))));
    assert!(harness.store.fresh.is_empty(), "a page was copied");

    assert_eq!(harness.remove_with(&6u32.to_be_bytes()), Some(vec![7; 40]));
    assert!(matches!(harness.root, Some(Child::Dirty { .. })));

    let copied = harness.store.fresh.len();

    assert_eq!(harness.remove_with(&3001u32.to_be_bytes()), None);
    assert_eq!(harness.store.fresh.len(), copied, "a page was copied");
    assert_eq!(
        harness.remove_with(&3000u32.to_be_bytes()),
        Some(vec![7; 40])
    );
    assert!(harness.store.fresh.len() > copied);
}

/// An insert whose visitor refuses the value it would replace stores
/// nothing, and gives back the pages it wrote for a value too long for a
/// leaf.
#[test]
fn a_refused_replacement_stores_nothing() {
    let mut harness = Harness::new(4096);

    for n in 0..500u32 {
        harness.insert(&n.to_be_bytes(), &[7; 40]);
    }

    harness.commit();

    let long = vec![9; 3 * 4096];

    assert!(harness.refuse_insert(&8u32.to_be_bytes(), &long));
    assert_eq!(harness.get(&8u32.to_be_bytes()), Some(vec![7; 40]));

    // What is left handed out is the path to the key, copied.
    let depth = harness.store.fresh.len();

    assert!(depth <= 3, "{depth} pages handed out");
    assert_eq!(
        harness.insert_with(&8u32.to_be_bytes(), &long),
        Some(vec![7; 40])
    );
    assert_eq!(harness.get(&8u32.to_be_bytes()), Some(long));
    harness.commit();
    assert_eq!(harness.check_structure(), 500);
}

/// Updates that grow values past a leaf, shrink them back into one, and
/// split leaves give back every page the values they replace had: once the
/// tree is emptied, every page ever handed out is free again.
#[test]
fn an_update_gives_back_what_the_value_it_replaces_took() {
    let mut rng = Rng::new(98);
    let mut harness = Harness::new(4096);
    let mut model = BTreeMap::new();
    let size = |rng: &mut Rng| match rng.below(6) {
        0 => 5000 + rng.index(9000),
        1 => 0,
        _ => rng.index(600),
    };

    for index in 0..1000u32 {
        let len = size(&mut rng);
        let value = rng.bytes(len);

        harness.insert(&index.to_be_bytes(), &value);
        model.insert(index.to_be_bytes().to_vec(), value);
    }

    harness.commit();

    for round in 0..6 {
        for _ in 0..400 {
            let key = u32::try_from(rng.below(1100)).unwrap().to_be_bytes();
            let len = size(&mut rng);
            let value = rng.bytes(len);
            let visited = harness.update(&key, Some(&value), false);

            assert_eq!(visited.as_ref(), model.get(&key[..]), "round {round}");

            if visited.is_some() {
                model.insert(key.to_vec(), value);
            }
        }

        harness.commit();
        assert_eq!(harness.check_structure(), model.len(), "round {round}");
    }

    for (key, value) in &model {
        assert_eq!(harness.get(key).as_ref(), Some(value));
        assert!(harness.remove(key));
    }

    harness.commit();

    let free: HashSet<u64> = harness.store.free.iter().copied().collect();

    assert_eq!(free.len(), harness.store.free.len(), "a page freed twice");
    assert_eq!(free, (1..harness.store.next).collect());
}

/// A change takes a committed node out of the page cache without a copy
/// when nothing else holds it, and copies one a reader holds, which stays as
/// it was, and in the cache.
#[test]
fn a_change_takes_a_cached_node_only_when_no_reader_holds_it() {
    let mut harness = Harness::new(4096);

    for key in 0..20u8 {
        harness.insert(&[key], &[key; 8]);
    }

    harness.commit();

    let Some(Child::Clean(root)) = harness.root.clone() else {
        panic!("a committed root");
    };
    let held = harness.loader.load(&root, TREE, None).unwrap();
    let before = held.to_node();

    assert!(harness.loader.cached(&root));
    harness.insert(&[100], &[1; 8]);
    assert_eq!(held.to_node(), before, "the reader's node is unchanged");
    assert!(
        harness.loader.cached(&root),
        "a node a reader holds stays cached"
    );

    drop(held);
    harness.commit();

    let Some(Child::Clean(root)) = harness.root.clone() else {
        panic!("a committed root");
    };

    // Read once, which caches it, and let go.
    drop(harness.loader.load(&root, TREE, None).unwrap());
    assert!(harness.loader.cached(&root));
    harness.insert(&[101], &[1; 8]);
    assert!(
        !harness.loader.cached(&root),
        "a node no one holds is taken"
    );
    assert_eq!(harness.get(&[101]), Some(vec![1; 8]));
    assert_eq!(harness.get(&[100]), Some(vec![1; 8]));
    assert_eq!(harness.get(&[5]), Some(vec![5; 8]));
}

/// A change copies a committed node written before the unsynced window, and
/// leaves it in the page cache for the readers that go on reading it.
#[test]
fn a_change_copies_a_node_older_than_the_window() {
    let mut harness = Harness::new(4096);

    for key in 0..20u8 {
        harness.insert(&[key], &[key; 8]);
    }

    harness.commit();

    let Some(Child::Clean(root)) = harness.root.clone() else {
        panic!("a committed root");
    };

    // A barrier made the commit durable: its pages are not young.
    harness.store.young_after = root.txn;

    drop(harness.loader.load(&root, TREE, None).unwrap());
    harness.insert(&[100], &[1; 8]);
    assert!(harness.loader.cached(&root), "an older node stays cached");
    assert_eq!(harness.get(&[100]), Some(vec![1; 8]));
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

        if let Node::Branch(branch) = &resolve(&harness.loader, &child, TREE, level)
            .unwrap()
            .to_node()
        {
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
    let Node::Leaf(leaf) = node.as_ref() else {
        panic!();
    };

    assert!(matches!(
        leaf.to_entries().unwrap()[0].value,
        StoredValue::Overflow(_)
    ));
    assert_eq!(harness.get(b"k"), Some(big.clone()));

    harness.commit();

    assert_eq!(harness.get(b"k"), Some(big));
}

/// Also: counting the entries of a range a leaf at a time gives the number
/// a walk gives, and lending them gives the same entries, both ways.
#[test]
fn a_backward_walk_gives_the_forward_walk_in_reverse() {
    let mut rng = Rng::new(11);

    for page_size in [4096, 16384] {
        let mut harness = Harness::new(page_size);

        for round in 0..3 {
            for _ in 0..600 {
                let key = key_of(&mut rng, page_size);

                if rng.below(4) == 0 {
                    harness.remove(&key);
                } else {
                    harness.insert(&key, &value_of(&mut rng, page_size));
                }
            }

            // Uncommitted nodes, then committed ones.
            if round > 0 {
                harness.commit();
            }

            for _ in 0..40 {
                let a = key_of(&mut rng, page_size);
                let b = key_of(&mut rng, page_size);
                let bound = |rng: &mut Rng, key: &[u8]| -> Bound<Vec<u8>> {
                    match rng.below(3) {
                        0 => Bound::Unbounded,
                        1 => Bound::Included(key.to_vec()),
                        _ => Bound::Excluded(key.to_vec()),
                    }
                };
                let (low, high) = if a <= b { (a, b) } else { (b, a) };
                let start = bound(&mut rng, &low);
                let end = bound(&mut rng, &high);
                let mut forward = harness.entries(as_slice(&start), as_slice(&end));
                let len = u64::try_from(forward.len()).unwrap();

                for backward in [false, true] {
                    assert_eq!(
                        harness.count(as_slice(&start), as_slice(&end), backward),
                        len,
                        "page size {page_size}, {start:?} to {end:?}, backward {backward}"
                    );
                }

                assert_eq!(
                    harness.lent(as_slice(&start), as_slice(&end), false),
                    forward,
                    "page size {page_size}, {start:?} to {end:?}"
                );

                forward.reverse();

                assert_eq!(
                    harness.lent(as_slice(&start), as_slice(&end), true),
                    forward,
                    "page size {page_size}, {start:?} to {end:?}, backward"
                );

                assert_eq!(
                    harness.entries_backward(as_slice(&start), as_slice(&end)),
                    forward,
                    "page size {page_size}, {start:?} to {end:?}"
                );
            }
        }
    }
}

fn as_slice(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    bound.as_ref().map(Vec::as_slice)
}
