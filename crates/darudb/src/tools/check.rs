//! The integrity check: everything one commit reaches, verified, and every
//! page of the file accounted for (`design/commits-and-recovery.md`,
//! "Checking and salvaging", and `design/tools.md`).
//!
//! The check reads the commit a read transaction sees, so it runs while
//! other handles and processes write: the snapshot keeps every page it reads
//! from being reused. It reports every problem it finds rather than stopping
//! at the first. A page it cannot read is a problem, and the pages below it
//! are skipped; the pages it could not reach are then left out of the count
//! of leaked pages, which would otherwise blame them too.

use std::ops::Bound;

use crate::btree::{Load, Loader, NodeRef};
use crate::database::Database;
use crate::error::Result;
use crate::format::object::codec;
use crate::format::object::names::{META, SCHEMA_KEY, counter, index_tree, records};
use crate::format::object::schema::StoredSchema;
use crate::format::{
    CATALOG_TREE, CommitRecord, FIRST_USER_TREE, FREE_TREE, Pointer, RETAINED_TREE, StoredRef,
    TreeDescriptor, decode_free_key, decode_free_value, decode_retained_key, decode_runs,
};
use crate::schema::objects::index_entries;
use crate::storage::Pager;
use crate::txn::ReadTransaction;

/// What the integrity check found: the commit it checked and every problem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckReport {
    /// The transaction id of the commit checked, the one published when the
    /// check began.
    pub commit_id: u64,
    /// The pages that commit counts, the header page included.
    pub page_count: u64,
    /// The pages read and verified.
    pub pages_checked: u64,
    /// The objects read and checked against their indexes.
    pub objects_checked: u64,
    /// Every problem found, in the order found. None when the file is whole.
    pub problems: Vec<Problem>,
}

impl CheckReport {
    /// Whether the check found nothing wrong.
    pub fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// One thing the integrity check found wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Problem {
    /// The page the problem is in, when it is in one page.
    pub page: Option<u64>,
    /// The tree it was found in, when it was found in one: a name as
    /// [`ReadTransaction::tree_names`] gives it, or a collection's name.
    pub tree: Option<String>,
    /// What is wrong.
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.page, &self.tree) {
            (Some(page), Some(tree)) => write!(f, "page {page} of {tree:?}: {}", self.message),
            (Some(page), None) => write!(f, "page {page}: {}", self.message),
            (None, Some(tree)) => write!(f, "{tree:?}: {}", self.message),
            (None, None) => f.write_str(&self.message),
        }
    }
}

/// Checks the commit a new read transaction of `db` sees.
pub(crate) fn check(db: &Database) -> Result<CheckReport> {
    let read = db.begin_read()?;
    let record = *read.record();
    let shared = db.shared();
    let mut checker = Checker::new(&shared.loader, &shared.pager, record);

    checker.kernel();
    checker.objects(&read);
    checker.accounting();

    Ok(checker.report)
}

/// What the walk of a tree gives each of its entries: the key and the value.
type Visit<'v, 'a> = dyn FnMut(&mut Checker<'a>, &[u8], &[u8]) + 'v;

/// A page still to read: its pointer, the level its parent expects, and the
/// keys it lies between, at or above the first and below the second.
type Pending = (Pointer, Option<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// A tree of the commit and where the check stands with it.
struct Walked {
    name: String,
    id: u64,
    root: Pointer,
    /// The entries its descriptor counts; none for the allocator trees and
    /// the catalog, which have no descriptor.
    counted: Option<u64>,
}

struct Checker<'a> {
    loader: &'a Loader,
    pager: &'a Pager,
    record: CommitRecord,
    /// One bit for every page of the commit: set once something uses it.
    claimed: Vec<u64>,
    /// Set when a page could not be read, so the pages below it were not
    /// claimed.
    incomplete: bool,
    free_runs: Vec<(u64, u64)>,
    retained_runs: Vec<(u64, u64)>,
    report: CheckReport,
}

impl<'a> Checker<'a> {
    fn new(loader: &'a Loader, pager: &'a Pager, record: CommitRecord) -> Self {
        let words = usize::try_from(record.page_count.div_ceil(64)).unwrap_or(usize::MAX);

        Self {
            loader,
            pager,
            record,
            claimed: vec![0; words],
            incomplete: false,
            free_runs: Vec::new(),
            retained_runs: Vec::new(),
            report: CheckReport {
                commit_id: record.txn,
                page_count: record.page_count,
                ..CheckReport::default()
            },
        }
    }

    fn problem(&mut self, page: Option<u64>, tree: Option<&str>, message: impl Into<String>) {
        self.report.problems.push(Problem {
            page,
            tree: tree.map(str::to_owned),
            message: message.into(),
        });
    }

    /// Marks `page` used by `tree`, and says whether it may be read: a page
    /// outside the file, or one used already, is a problem.
    fn claim(&mut self, page: u64, tree: &str) -> bool {
        if page == 0 || page >= self.record.page_count {
            self.problem(
                Some(page),
                Some(tree),
                format!(
                    "lies outside the {} pages of the file",
                    self.record.page_count
                ),
            );

            return false;
        }

        let (word, bit) = (usize::try_from(page / 64).unwrap_or(usize::MAX), page % 64);

        if self.claimed[word] & (1 << bit) != 0 {
            self.problem(Some(page), Some(tree), "is used twice");

            return false;
        }

        self.claimed[word] |= 1 << bit;

        true
    }

    /// Walks every tree of the commit: the catalog, the free tree, the
    /// retained tree, and every tree the catalog names.
    fn kernel(&mut self) {
        let record = self.record;
        let mut trees = vec![
            Walked {
                name: "the catalog".to_owned(),
                id: CATALOG_TREE,
                root: record.catalog,
                counted: None,
            },
            Walked {
                name: "the free tree".to_owned(),
                id: FREE_TREE,
                root: record.free,
                counted: None,
            },
            Walked {
                name: "the retained tree".to_owned(),
                id: RETAINED_TREE,
                root: record.retained,
                counted: None,
            },
        ];
        let mut next = 0;

        while next < trees.len() {
            let tree = &trees[next];
            let (name, id, root, counted) = (tree.name.clone(), tree.id, tree.root, tree.counted);
            let mut found = Vec::new();
            let walked = self.walk(&name, id, root, &mut |checker, key, value| {
                checker.entry(&name, id, key, value, &mut found);
            });

            if let (Some(walked), Some(counted)) = (walked, counted) {
                if walked != counted {
                    self.problem(
                        None,
                        Some(&name),
                        format!("holds {walked} entries where its descriptor counts {counted}"),
                    );
                }
            }

            trees.extend(found);
            next += 1;
        }
    }

    /// Checks one entry of tree `id`, and gives the catalog's trees to
    /// `found` to walk.
    fn entry(&mut self, name: &str, id: u64, key: &[u8], value: &[u8], found: &mut Vec<Walked>) {
        match id {
            CATALOG_TREE => match TreeDescriptor::decode(value) {
                Ok(descriptor) => {
                    let tree = String::from_utf8_lossy(key).into_owned();

                    if descriptor.root.is_null() != (descriptor.entries == 0) {
                        self.problem(
                            None,
                            Some(&tree),
                            "counts entries its root does not have, or none its root has",
                        );
                    }

                    if descriptor.id < FIRST_USER_TREE {
                        self.problem(None, Some(&tree), "has the id of an allocator tree");
                    }

                    found.push(Walked {
                        name: tree,
                        id: descriptor.id,
                        root: descriptor.root,
                        counted: Some(descriptor.entries),
                    });
                }
                Err(reason) => self.problem(None, Some(name), reason),
            },
            FREE_TREE => match (decode_free_key(key), decode_free_value(value)) {
                (Ok(start), Ok(len)) => self.free_runs.push((start, len)),
                (Err(reason), _) | (_, Err(reason)) => self.problem(None, Some(name), reason),
            },
            RETAINED_TREE => match (decode_retained_key(key), decode_runs(value)) {
                (Ok((group, _)), Ok(runs)) => {
                    if group > self.record.txn {
                        self.problem(
                            None,
                            Some(name),
                            format!(
                                "holds a group of commit {group}, newer than the commit checked"
                            ),
                        );
                    }

                    self.retained_runs
                        .extend(runs.into_iter().map(|(start, len)| (start, u64::from(len))));
                }
                (Err(reason), _) | (_, Err(reason)) => self.problem(None, Some(name), reason),
            },
            _ => {}
        }
    }

    /// Reads every page of the tree whose root is `root`, checking that each
    /// key lies between the separators on its path, and gives `visit` each
    /// entry with its value. Returns how many entries it read, or `None`
    /// when a page could not be read.
    fn walk(
        &mut self,
        name: &str,
        tree: u64,
        root: Pointer,
        visit: &mut Visit<'_, 'a>,
    ) -> Option<u64> {
        if root.is_null() {
            return Some(0);
        }

        let mut whole = true;
        let mut entries = 0u64;
        let mut pending: Vec<Pending> = vec![(root, None, None, None)];

        while let Some((pointer, level, low, high)) = pending.pop() {
            if !self.claim(pointer.page, name) {
                whole = false;
                self.incomplete = true;

                continue;
            }

            let loaded = match self.loader.load(&pointer, tree, level) {
                Ok(loaded) => loaded,
                Err(error) => {
                    self.problem(Some(pointer.page), Some(name), error.to_string());
                    whole = false;
                    self.incomplete = true;

                    continue;
                }
            };

            self.report.pages_checked += 1;

            let level = loaded.level();
            let node = NodeRef::Loaded(loaded);
            let count = node.count();
            let outside = |key: &[u8]| {
                low.as_deref().is_some_and(|low| key < low)
                    || high.as_deref().is_some_and(|high| key >= high)
            };

            if (0..count).any(|index| outside(node.key(index))) {
                self.problem(
                    Some(pointer.page),
                    Some(name),
                    "holds a key outside the separators on its path",
                );
            }

            if !node.is_leaf() {
                let NodeRef::Loaded(loaded) = &node else {
                    continue;
                };

                for index in 0..=count {
                    let child_low = match index {
                        0 => low.clone(),
                        _ => Some(node.key(index - 1).to_vec()),
                    };
                    let child_high = if index == count {
                        high.clone()
                    } else {
                        Some(node.key(index).to_vec())
                    };

                    pending.push((loaded.child(index), Some(level - 1), child_low, child_high));
                }

                continue;
            }

            for index in 0..count {
                entries += 1;

                let (key, value) = match node.entry(index) {
                    Ok(entry) => entry,
                    Err(error) => {
                        self.problem(Some(pointer.page), Some(name), error.to_string());

                        continue;
                    }
                };

                match value {
                    StoredRef::Inline(value) => visit(self, key, value),
                    StoredRef::Overflow(reference) => {
                        let first = reference.first;
                        let claimed = (0..u64::from(reference.pages))
                            .all(|at| self.claim(reference.first + at, name));

                        if !claimed {
                            self.incomplete = true;

                            continue;
                        }

                        match self.loader.read_overflow(&reference, tree) {
                            Ok(value) => {
                                self.report.pages_checked += u64::from(reference.pages);
                                visit(self, key, &value);
                            }
                            Err(error) => {
                                // The page that fails on its own, where one
                                // does; the run's first page otherwise.
                                let failed = (first..first + u64::from(reference.pages))
                                    .find(|page| {
                                        self.pager.read_run_self_checked(*page, 1).is_err()
                                    })
                                    .unwrap_or(first);

                                self.problem(Some(failed), Some(name), error.to_string());
                            }
                        }
                    }
                }
            }
        }

        whole.then_some(entries)
    }

    /// Checks the free and retained runs, and that every page of the file
    /// is used, free or retained, exactly once.
    fn accounting(&mut self) {
        for (runs, what) in [
            (std::mem::take(&mut self.free_runs), "the free pages"),
            (
                std::mem::take(&mut self.retained_runs),
                "the retained pages",
            ),
        ] {
            for (start, len) in runs {
                let end = start.checked_add(len);

                if len == 0 || end.is_none_or(|end| end > self.record.page_count) {
                    self.problem(
                        Some(start),
                        Some(what),
                        format!("a run of {len} pages lies outside the file"),
                    );

                    continue;
                }

                for page in start..start + len {
                    self.claim(page, what);
                }
            }
        }

        let mut leaked = 0u64;
        let mut first = None;

        for page in 1..=self.record.page_count {
            let used = page == self.record.page_count || {
                let (word, bit) = (usize::try_from(page / 64).unwrap_or(usize::MAX), page % 64);

                self.claimed[word] & (1 << bit) != 0
            };

            match (used, first) {
                (false, None) => first = Some(page),
                (true, Some(start)) => {
                    leaked += page - start;

                    if !self.incomplete {
                        let message = if page - start == 1 {
                            "is neither used, free nor retained: it leaked".to_owned()
                        } else {
                            format!(
                                "and the {} pages after it are neither used, free nor retained: they leaked",
                                page - start - 1
                            )
                        };

                        self.problem(Some(start), None, message);
                    }

                    first = None;
                }
                _ => {}
            }
        }

        if self.incomplete && leaked > 0 {
            self.problem(
                None,
                None,
                format!(
                    "{leaked} pages were not reached, some of them below pages that could not be read"
                ),
            );
        }
    }

    /// Checks the object layer, if the file holds a schema: every record
    /// decodes and is stored under its key, every index holds exactly the
    /// entries its collection's objects give it, and every auto-increment
    /// counter lies past every key it numbered.
    fn objects(&mut self, read: &ReadTransaction) {
        let stored = match read.get_in(META, SCHEMA_KEY) {
            Ok(Some(stored)) => stored,
            Ok(None) => return,
            Err(error) => {
                self.problem(None, Some(META), format!("its schema: {error}"));

                return;
            }
        };
        let schema = match StoredSchema::decode(&stored) {
            Ok(schema) => schema,
            Err(reason) => {
                let reason = reason.unwrap_or("it is in an object format this build does not read");

                self.problem(None, Some(META), format!("its schema: {reason}"));

                return;
            }
        };

        for collection in &schema.collections {
            let name = collection.name.as_str();
            let tree = records(collection.id);
            let range = match read.range_in::<&[u8]>(
                &tree,
                &(Bound::<&[u8]>::Unbounded, Bound::<&[u8]>::Unbounded),
                false,
            ) {
                Ok(range) => range,
                Err(error) => {
                    self.problem(None, Some(name), error.to_string());

                    continue;
                }
            };
            let mut expected = vec![0u64; collection.indexes.len()];
            let mut largest: Option<i64> = None;

            for entry in range {
                let (key, bytes) = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        self.problem(None, Some(name), error.to_string());

                        break;
                    }
                };

                self.report.objects_checked += 1;

                let object = match codec::object_of(&bytes, &collection.fields) {
                    Ok(object) => object,
                    Err(reason) => {
                        self.problem(None, Some(name), format!("an object's record: {reason}"));

                        continue;
                    }
                };
                let key_value = collection
                    .key_field()
                    .and_then(|field| object.get(&field.name));
                let stored_under = key_value
                    .map(crate::format::object::key::encoded)
                    .and_then(std::result::Result::ok);

                if stored_under.as_deref() != Some(key.as_slice()) {
                    self.problem(
                        None,
                        Some(name),
                        "an object is stored under another key than its own",
                    );
                }

                if let Some(crate::Value::Int(id)) = key_value {
                    largest = largest.max(Some(*id));
                }

                for (position, index) in collection.indexes.iter().enumerate() {
                    let entries = match index_entries(index, collection, &object, &key) {
                        Ok(entries) => entries,
                        Err(error) => {
                            self.problem(None, Some(name), error.to_string());

                            continue;
                        }
                    };

                    expected[position] += entries.len() as u64;

                    for (entry, value) in entries {
                        match read.get_in(&index_tree(index.id), &entry) {
                            Ok(Some(held)) if held == value => {}
                            Ok(Some(_)) => self.problem(
                                None,
                                Some(name),
                                format!(
                                    "index {} names another object for one of its entries",
                                    index.id
                                ),
                            ),
                            Ok(None) => self.problem(
                                None,
                                Some(name),
                                format!(
                                    "index {} lacks an entry one of its objects gives it",
                                    index.id
                                ),
                            ),
                            Err(error) => self.problem(None, Some(name), error.to_string()),
                        }
                    }
                }
            }

            // Every entry the objects give each index is there, so an index
            // holding as many entries as they give holds nothing else.
            for (position, index) in collection.indexes.iter().enumerate() {
                match read.len_in(&index_tree(index.id)) {
                    Ok(held) if held == expected[position] => {}
                    Ok(held) => self.problem(
                        None,
                        Some(name),
                        format!(
                            "index {} holds {held} entries where its objects give {}",
                            index.id, expected[position]
                        ),
                    ),
                    Err(error) => self.problem(None, Some(name), error.to_string()),
                }
            }

            if collection.auto {
                let next = match read.get_in(META, counter(collection.id).as_bytes()) {
                    Ok(Some(bytes)) => bytes.as_slice().try_into().ok().map(u64::from_le_bytes),
                    Ok(None) => Some(1),
                    Err(error) => {
                        self.problem(None, Some(name), error.to_string());

                        continue;
                    }
                };

                match (next, largest) {
                    (None, _) => self.problem(
                        None,
                        Some(name),
                        "its auto-increment counter is not 8 bytes long",
                    ),
                    (Some(next), Some(largest))
                        if i64::try_from(next).is_ok_and(|next| next <= largest) =>
                    {
                        self.problem(
                            None,
                            Some(name),
                            format!(
                                "its auto-increment counter, {next}, would give a key already used"
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::format::object::key;
    use crate::storage::sim::SimDisk;
    use crate::{Collection, Object, OpenOptions, Schema, Type, Value};

    fn schema() -> Schema {
        Schema::new(1).collection(
            Collection::new("people")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email")
                .index("age"),
        )
    }

    /// A database of 300 people and a large value, on a simulated disk,
    /// plain or encrypted.
    fn filled(encrypted: bool) -> (Arc<SimDisk>, OpenOptions, Database) {
        let disk = Arc::new(SimDisk::default());
        let mut options = OpenOptions::new();

        options.schema(schema());

        if encrypted {
            options.key([9; 32]);
        }

        let db = Database::create_io(disk.clone(), 4096, &options).unwrap();
        let mut txn = db.begin_write().unwrap();

        {
            let mut people = txn.collection("people").unwrap();

            for n in 0..300 {
                people
                    .insert(
                        Object::new()
                            .with("name", format!("person {n}"))
                            .with("email", format!("{n}@example.com"))
                            .with("age", n % 40),
                    )
                    .unwrap();
            }
        }

        txn.insert("blobs", b"large", &vec![5; 20_000]).unwrap();
        txn.commit().unwrap();

        (disk, options, db)
    }

    fn messages(report: &CheckReport) -> String {
        report
            .problems
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_whole_file_passes_and_everything_in_it_is_read() {
        for encrypted in [false, true] {
            let (_, _, db) = filled(encrypted);
            let report = db.check().unwrap();

            assert!(report.is_ok(), "{}", messages(&report));
            assert_eq!(report.objects_checked, 300);
            assert!(report.pages_checked > 10, "{report:?}");
            assert_eq!(
                report.page_count,
                db.shared().header().published().unwrap().page_count
            );
        }
    }

    /// Each page the commit reaches, damaged in turn, is named by the
    /// check, which goes on to report the rest and never panics.
    #[test]
    fn a_damaged_page_is_named() {
        for encrypted in [false, true] {
            let (disk, options, db) = filled(encrypted);
            let page_count = db.shared().header().published().unwrap().page_count;

            drop(db);

            let image = disk.current();
            let mut named = 0;

            for page in 1..page_count {
                let mut damaged = image.clone();
                let at = usize::try_from(page).unwrap() * 4096 + 2000;

                damaged[at] ^= 0x55;

                let db = Database::open_io(Arc::new(SimDisk::from_image(damaged)), &options);
                // Damage to a tree the open reads, the catalog's or the
                // schema's, may stop it opening at all.
                let Ok(db) = db else {
                    continue;
                };
                let report = db.check().unwrap();

                if report
                    .problems
                    .iter()
                    .any(|problem| problem.page == Some(page))
                {
                    named += 1;
                } else {
                    // A free or retained page is not read.
                    assert!(report.is_ok(), "page {page}: {}", messages(&report));
                }
            }

            assert!(named > 5, "{named} damaged pages named");
        }
    }

    /// An index entry gone, one too many, and an object under another key
    /// than its own, each in a file of its own.
    #[test]
    fn an_index_that_disagrees_with_its_objects_is_reported() {
        for case in 0..3 {
            let (_, _, db) = filled(false);
            let open = db.begin_read().unwrap().schema().cloned().unwrap();
            let people = &open.schema.collections[0];
            let (email, age) = (people.indexes[0].id, people.indexes[1].id);
            let mut txn = db.begin_write().unwrap();
            let expected = match case {
                0 => {
                    let entry = key::encoded(&Value::from("7@example.com")).unwrap();

                    assert!(txn.remove_in(&index_tree(email), &entry).unwrap());

                    vec![
                        format!("index {email} lacks an entry one of its objects gives it"),
                        format!("index {email} holds 299 entries where its objects give 300"),
                    ]
                }
                1 => {
                    txn.insert_in(&index_tree(age), b"no object gives this", b"")
                        .unwrap();

                    vec![format!(
                        "index {age} holds 301 entries where its objects give 300"
                    )]
                }
                _ => {
                    let from = key::encoded(&Value::Int(7)).unwrap();
                    let record = txn.get_in(&records(people.id), &from).unwrap().unwrap();
                    let to = key::encoded(&Value::Int(8)).unwrap();

                    txn.insert_in(&records(people.id), &to, &record).unwrap();

                    vec!["an object is stored under another key than its own".to_owned()]
                }
            };

            txn.commit().unwrap();

            let report = db.check().unwrap();
            let found: Vec<&str> = report
                .problems
                .iter()
                .map(|problem| problem.message.as_str())
                .collect();

            for message in &expected {
                assert!(found.contains(&message.as_str()), "case {case}: {found:?}");
            }

            assert!(
                report
                    .problems
                    .iter()
                    .all(|problem| problem.tree.as_deref() == Some("people")),
                "case {case}: {found:?}"
            );
        }
    }

    /// A branch whose two first children trade places, and a tree whose
    /// descriptor counts one entry too many, with every check above them
    /// made to match: each page verifies, but the keys of each child lie on
    /// the wrong side of the separator between them, or the count is wrong.
    #[test]
    fn keys_outside_their_separators_and_a_wrong_count_are_reported() {
        for swap in [true, false] {
            separators_or_count(swap);
        }
    }

    fn separators_or_count(swap: bool) {
        use crate::format::{CONTENT_OFFSET, POINTER_LEN, Pointer, seal, slot_offset};

        let disk = Arc::new(SimDisk::default());
        let db = Database::create_io(disk.clone(), 4096, &OpenOptions::new()).unwrap();
        let mut txn = db.begin_write().unwrap();

        for n in 0u32..600 {
            txn.insert("t", &n.to_be_bytes(), &[1; 24]).unwrap();
        }

        txn.commit().unwrap();

        let header = db.shared().header();
        let mut record = header.published().unwrap();
        let slot = header.selector.slot;
        let mut image = disk.current();
        let page_of = |image: &mut Vec<u8>, page: u64| {
            let at = usize::try_from(page).unwrap() * 4096;

            image[at..at + 4096].to_vec()
        };
        let put_page = |image: &mut Vec<u8>, page: u64, bytes: &[u8]| {
            let at = usize::try_from(page).unwrap() * 4096;

            image[at..at + 4096].copy_from_slice(bytes);
        };

        drop(db);

        // The catalog is one leaf, whose entry for `t` names its root.
        let catalog_page = record.catalog.page;
        let mut catalog = page_of(&mut image, catalog_page);
        let header = crate::format::PageHeader::read(&catalog).unwrap();
        let (at, len, old) = (0..usize::from(header.count))
            .find_map(
                |index| match crate::format::leaf_entry(&catalog, index).unwrap() {
                    (b"t", StoredRef::Inline(value)) => Some((
                        value.as_ptr() as usize - catalog.as_ptr() as usize,
                        value.len(),
                        TreeDescriptor::decode(value).unwrap(),
                    )),
                    _ => None,
                },
            )
            .unwrap();
        let root = old.root.page;
        let mut descriptor = TreeDescriptor {
            entries: old.entries + 1,
            ..old
        };

        if swap {
            let mut branch = page_of(&mut image, root);
            let first = CONTENT_OFFSET;
            let second = CONTENT_OFFSET + POINTER_LEN;
            let (left, right) = (
                Pointer::read(&branch[first..]),
                Pointer::read(&branch[second..]),
            );

            right.write(&mut branch[first..]);
            left.write(&mut branch[second..]);

            let check = seal(root, &mut branch);

            put_page(&mut image, root, &branch);

            descriptor = TreeDescriptor {
                root: Pointer { check, ..old.root },
                ..old
            };
        }

        catalog[at..at + len].copy_from_slice(&descriptor.encode());
        record.catalog.check = seal(catalog_page, &mut catalog);
        put_page(&mut image, catalog_page, &catalog);

        let at = slot_offset(slot);

        image[at..at + crate::format::RECORD_LEN].copy_from_slice(&record.encode(slot));

        let db =
            Database::open_io(Arc::new(SimDisk::from_image(image)), &OpenOptions::new()).unwrap();
        let report = db.check().unwrap();

        let expected = if swap {
            "holds a key outside the separators on its path"
        } else {
            "holds 600 entries where its descriptor counts 601"
        };

        assert!(
            report
                .problems
                .iter()
                .any(|problem| problem.tree.as_deref() == Some("t") && problem.message == expected),
            "{}",
            messages(&report)
        );
    }

    #[test]
    fn a_counter_that_would_give_a_key_again_is_reported() {
        let (_, _, db) = filled(false);
        let id = db
            .begin_read()
            .unwrap()
            .schema()
            .cloned()
            .unwrap()
            .schema
            .collections[0]
            .id;
        let mut txn = db.begin_write().unwrap();

        txn.insert_in(META, counter(id).as_bytes(), &5u64.to_le_bytes())
            .unwrap();
        txn.commit().unwrap();

        let text = messages(&db.check().unwrap());

        assert!(
            text.contains("counter, 5, would give a key already used"),
            "{text}"
        );
    }

    /// The accounting, on pages claimed by hand: a page used twice, runs
    /// outside the file, and leaked pages, one alone and a run of them.
    #[test]
    fn every_page_is_counted_exactly_once() {
        let (_, _, db) = filled(false);
        let mut record = db.shared().header().published().unwrap();

        record.page_count = 40;

        let shared = db.shared();
        let mut checker = Checker::new(&shared.loader, &shared.pager, record);

        for page in (1..20).chain(22..30) {
            assert!(checker.claim(page, "a tree"));
        }

        assert!(!checker.claim(5, "another tree"));
        assert!(!checker.claim(40, "another tree"));
        checker.free_runs.push((30, 5));
        checker.free_runs.push((38, 5));
        checker.retained_runs.push((29, 1));
        checker.accounting();

        let problems: Vec<(Option<u64>, &str)> = checker
            .report
            .problems
            .iter()
            .map(|problem| (problem.page, problem.message.as_str()))
            .collect();

        assert_eq!(
            problems,
            [
                (Some(5), "is used twice"),
                (Some(40), "lies outside the 40 pages of the file"),
                (Some(38), "a run of 5 pages lies outside the file"),
                (Some(29), "is used twice"),
                (
                    Some(20),
                    "and the 1 pages after it are neither used, free nor retained: they leaked"
                ),
                (
                    Some(35),
                    "and the 4 pages after it are neither used, free nor retained: they leaked"
                ),
            ]
        );
    }
}
