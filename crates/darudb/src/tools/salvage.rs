//! Salvage: what can be rescued from a damaged file, written into a new one
//! (`design/tools.md`, "Salvage").
//!
//! Salvage reads the file page by page rather than opening it as a
//! database, so neither a damaged header nor a damaged tree stops it. It
//! scans every page and notes the leaves that verify; walks the newest
//! commit it can use, as the integrity check does, copying every entry it
//! reads; and fills what that commit could not read from the leaves of older
//! commits, newest first. The new file's objects then get their indexes built
//! again, so that the file passes the integrity check whatever was lost.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::ops::Bound;
use std::path::Path;
use std::sync::Arc;

use super::{COMMIT_BYTES, io_error, taken, write_new};
use crate::Value;
use crate::btree::{Load, LoadedNode, Loader, NodeRef};
use crate::crypto::{DataKey, PageCipher, RecordAuth};
use crate::database::{Database, Held, create_beside, header_error, hold_alone, unlock};
use crate::error::{Error, Result};
use crate::format::object::names::{
    INDEX_PREFIX, META, RECORDS_PREFIX, SCHEMA_KEY, counter, index_tree, records,
};
use crate::format::object::schema::{CollectionDef, StoredSchema};
use crate::format::object::{codec, key};
use crate::format::{
    CATALOG_TREE, Cipher, CommitRecord, FIRST_USER_TREE, HEADER_LEN, HeaderError, KEY_BLOCK_LEN,
    MAX_PAGE_SIZE, MIN_PAGE_SIZE, PageHeader, Pointer, SLOT_COUNT, StaticHeader, StoredRef,
    TreeDescriptor, page_check, slot_offset, stored_check,
};
use crate::options::OpenOptions;
use crate::schema::objects::index_entries;
use crate::storage::{Cache, FileIo, Pager};
use crate::txn::WriteTransaction;

/// The bytes of nodes the salvage keeps cached while it reads.
const CACHE_BYTES: usize = 8 << 20;

/// How many of the first pages of a file whose static fields fail their
/// check are tried at each page size, to find the one the file has.
const PROBE_PAGES: u64 = 8;

/// How many objects each write transaction of an index rebuild reads.
const REBUILD_BATCH: usize = 1024;

/// What a salvage rescued, and what it could not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SalvageReport {
    /// The transaction id of the commit salvage started from, or none when
    /// no commit record could be used and every tree came from the pages the
    /// scan found.
    pub commit_id: Option<u64>,
    /// The pages of the file the scan read, the header page left out.
    pub pages_scanned: u64,
    /// The pages that failed their check, other than pages of zeros, which
    /// were never written.
    pub pages_damaged: u64,
    /// The pages of the commit that could not be read, each overflow value
    /// counted as one page: the keys under each were taken from older
    /// versions of the same pages, where the file still had them.
    pub pages_unread: u64,
    /// The entries taken from those older versions.
    pub entries_recovered: u64,
    /// The keys left out because no version of their value could be read.
    pub values_lost: u64,
    /// The objects left out: records that did not decode or were stored
    /// under another key than their own, objects whose entry in a unique
    /// index another object had taken, and every object of a file whose
    /// stored schema was lost.
    pub objects_dropped: u64,
    /// The trees of the new file, the engine's own included.
    pub trees: u64,
    /// The entries of the new file, the indexes' included.
    pub entries: u64,
    /// The size of the new file, in bytes.
    pub bytes: u64,
}

impl SalvageReport {
    /// Whether the new file holds exactly the commit salvage started from:
    /// every page of it was read, and no object was dropped.
    pub fn is_whole(&self) -> bool {
        self.commit_id.is_some() && self.pages_unread == 0 && self.objects_dropped == 0
    }
}

/// Rescues what it can of the file at `from` into a new database at `into`.
pub(crate) fn salvage(from: &Path, into: &Path, options: &OpenOptions) -> Result<SalvageReport> {
    if fs::symlink_metadata(into).is_ok() {
        return Err(taken(into, "salvage"));
    }

    let input = Input::open(from, options)?;

    // The test of opening a file under salvage stops it here.
    #[cfg(test)]
    crate::testing::pause_in_salvage(from);

    let mut report = SalvageReport::default();
    let salvager = Salvager {
        leaves: input.scan(&mut report),
        input: &input,
    };
    // The newest commit the file records. Where its pages are damaged, the
    // older versions of those pages are what an older record would give.
    let record = input.records.first().copied();
    let copy = create_beside(into, input.header, input.key_block, input.data_key.as_ref())?;

    report.commit_id = record.map(|record| record.txn);

    let ((), bytes) = write_new(into, "salvage", copy, |copy| {
        let names = salvager.copy(record.as_ref(), copy, &mut report)?;

        repair(copy, &names, &mut report)?;
        count(copy, &mut report)
    })?;

    report.bytes = bytes;

    Ok(report)
}

/// The damaged file, held alone, and what its header gave.
struct Input {
    loader: Loader,
    pager: Arc<Pager>,
    /// The static fields for the new file: the file's own, or for a plain
    /// file whose own fail their check, the page size its pages have and a
    /// new file id.
    header: StaticHeader,
    data_key: Option<DataKey>,
    /// The key block that the secret unwrapped, for the new file.
    key_block: [u8; KEY_BLOCK_LEN],
    /// The commit records that can be used, newest first.
    records: Vec<CommitRecord>,
    /// The pages of the file, page 0 included.
    pages: u64,
    /// Declared last, so that it lets go once the file is closed.
    _held: Held,
}

impl Input {
    fn open(path: &Path, options: &OpenOptions) -> Result<Self> {
        let held = hold_alone(path, options.settings().busy_timeout)?;
        let file: Arc<dyn FileIo> = held.file.clone();
        let len = file.len().map_err(|source| io_error(path, source))?;
        let mut bytes = vec![0u8; HEADER_LEN];
        let head = usize::try_from(len).map_or(HEADER_LEN, |len| len.min(HEADER_LEN));

        file.read_at(&mut bytes[..head], 0)
            .map_err(|source| io_error(path, source))?;

        let header = match StaticHeader::decode(&bytes) {
            Ok(header) => header,
            Err(error @ HeaderError::UnsupportedVersion(_)) => {
                return Err(header_error(path, error));
            }
            // An encrypted file's key block is bound to its file id, so only
            // a plain file can do without its static fields.
            Err(error) if options.secret().is_some() => return Err(header_error(path, error)),
            Err(error) => {
                let page_size =
                    page_size_found(&*file, len).ok_or_else(|| header_error(path, error))?;
                let mut file_id = [0u8; 16];

                getrandom::fill(&mut file_id)
                    .map_err(|error| io_error(path, std::io::Error::other(error)))?;

                StaticHeader {
                    version: options.new_format_version(),
                    page_size,
                    file_id,
                    cipher: Cipher::Plain,
                }
            }
        };
        let (data_key, key_block) = match (header.cipher, options.secret()) {
            (Cipher::Plain, Some(_)) => {
                return Err(Error::InvalidArgument {
                    message: format!(
                        "`{}` is not encrypted, so it cannot be salvaged with a key",
                        path.display()
                    ),
                });
            }
            (Cipher::Plain, None) => (None, [0; KEY_BLOCK_LEN]),
            _ => {
                let (data_key, key_block) = unlock(path, &header, &bytes, options)?;

                (Some(data_key), key_block)
            }
        };
        let records = usable_records(&bytes, &header, data_key.as_ref());
        let pager = Arc::new(Pager::new(
            file,
            header.page_size as usize,
            path.to_path_buf(),
            data_key
                .as_ref()
                .and_then(|key| PageCipher::new(header.cipher, key)),
        ));
        let loader = Loader::new(Arc::clone(&pager), Arc::new(Cache::new(CACHE_BYTES, 16)));

        Ok(Self {
            loader,
            pager,
            header,
            data_key,
            key_block,
            records,
            pages: len / u64::from(header.page_size),
            _held: held,
        })
    }

    /// Reads every page from 1 to the end of the file, and returns the
    /// leaves that verify, by tree, newest first.
    fn scan(&self, report: &mut SalvageReport) -> HashMap<u64, Vec<Leaf>> {
        let page_size = self.pager.page_size();
        let run = self.pager.run_pages();
        let mut leaves: HashMap<u64, Vec<Leaf>> = HashMap::new();
        let mut first = 1;

        while first < self.pages {
            let count = usize::try_from(self.pages - first).map_or(run, |left| left.min(run));

            match self.pager.read_run_unverified(first, count) {
                Ok(mut bytes) => {
                    for (number, page) in (first..).zip(bytes.chunks_mut(page_size)) {
                        self.note(number, page, &mut leaves, report);
                    }
                }
                // Read again a page at a time, so that only the pages that
                // cannot be read count as damaged.
                Err(_) => {
                    for number in (first..).take(count) {
                        match self.pager.read_run_unverified(number, 1) {
                            Ok(mut page) => self.note(number, &mut page, &mut leaves, report),
                            Err(_) => {
                                report.pages_scanned += 1;
                                report.pages_damaged += 1;
                            }
                        }
                    }
                }
            }

            first += count as u64;
        }

        for versions in leaves.values_mut() {
            versions.sort_by_key(|leaf| std::cmp::Reverse(leaf.pointer.txn));
        }

        leaves
    }

    /// Notes page `number`, whose bytes as the file holds them are `page`:
    /// a leaf that verifies goes into `leaves`.
    fn note(
        &self,
        number: u64,
        page: &mut [u8],
        leaves: &mut HashMap<u64, Vec<Leaf>>,
        report: &mut SalvageReport,
    ) {
        report.pages_scanned += 1;

        // The file grew past a page of zeros without writing it.
        if page.iter().all(|&byte| byte == 0) {
            return;
        }

        let Some(check) = self.pager.open(number, page) else {
            report.pages_damaged += 1;

            return;
        };
        let header = match PageHeader::read(page) {
            Ok(header) if header.kind.is_leaf() => header,
            Ok(_) => return,
            Err(_) => {
                report.pages_damaged += 1;

                return;
            }
        };
        let Ok(node) = LoadedNode::read(page.to_vec(), &header) else {
            report.pages_damaged += 1;

            return;
        };
        let node = NodeRef::Loaded(Arc::new(node));
        let count = node.count();

        if count == 0 {
            return;
        }

        leaves.entry(header.tree).or_default().push(Leaf {
            pointer: Pointer {
                page: number,
                txn: header.txn,
                check,
            },
            first: node.key(0).to_vec(),
            last: node.key(count - 1).to_vec(),
        });
    }
}

/// The page size of a plain file whose static fields fail their check: the
/// one at which the most of its first pages verify, if any does.
fn page_size_found(file: &dyn FileIo, len: u64) -> Option<u32> {
    let mut best: Option<(u32, usize)> = None;
    let mut size = MIN_PAGE_SIZE;

    while size <= MAX_PAGE_SIZE {
        let mut page = vec![0u8; size as usize];
        let pages = (len / u64::from(size)).min(PROBE_PAGES + 1);
        let verified = (1..pages)
            .filter(|&number| {
                file.read_at(&mut page, number * u64::from(size)).is_ok()
                    && page_check(number, &page) == stored_check(&page)
            })
            .count();

        if verified > 0 && best.is_none_or(|(_, most)| verified > most) {
            best = Some((size, verified));
        }

        size *= 2;
    }

    best.map(|(size, _)| size)
}

/// The commit records of the file that can be used, newest first: those
/// that pass their check and, in an encrypted file, their MAC. `bytes` is
/// the start of the file, [`HEADER_LEN`] bytes of it.
fn usable_records(
    bytes: &[u8],
    header: &StaticHeader,
    data_key: Option<&DataKey>,
) -> Vec<CommitRecord> {
    let auth = data_key.map(RecordAuth::new);
    let mut records: Vec<CommitRecord> = (0..SLOT_COUNT)
        .filter_map(|slot| {
            let record = CommitRecord::decode(slot, &bytes[slot_offset(slot)..])
                .ok()
                .flatten()?;
            let signed = auth.as_ref().is_none_or(|auth| {
                auth.verify(&header.file_id, slot, &record.authenticated(), &record.mac)
            });

            signed.then_some(record)
        })
        .collect();

    records.sort_by_key(|record| std::cmp::Reverse(record.txn));
    records
}

/// A leaf the scan found intact: its commit's version of the keys from its
/// first to its last, since the leaves of one version of a tree divide its
/// keys between them.
struct Leaf {
    pointer: Pointer,
    first: Vec<u8>,
    last: Vec<u8>,
}

/// What a walk gives each entry it reads whole: the key and the value.
type Visit<'v> = dyn FnMut(&[u8], &[u8]) -> Result<()> + 'v;

/// A page still to read: its pointer, the level its parent expects, and the
/// keys it lies between, at or above the first and below the second.
type Pending = (Pointer, Option<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// Keys of one tree that the commit could not read.
struct Gap {
    tree: u64,
    /// The keys the gap covers are at or above this one...
    low: Option<Vec<u8>>,
    /// ...and below this one.
    high: Option<Vec<u8>>,
    /// The commit that wrote the page that was lost. No page below it is
    /// newer, so a leaf of a newer commit is no version of its keys.
    newest: u64,
    /// Keys the gap is known to hold whose value is still to be found.
    wanted: BTreeSet<Vec<u8>>,
}

impl Gap {
    /// Every key of `tree`, of any commit.
    fn everything(tree: u64) -> Self {
        Self {
            tree,
            low: None,
            high: None,
            newest: u64::MAX,
            wanted: BTreeSet::new(),
        }
    }

    /// The value of `key` alone, which a leaf of commit `newest` holds but
    /// could not be read.
    fn value(tree: u64, key: &[u8], newest: u64) -> Self {
        let mut high = key.to_vec();

        // The next key after `key`.
        high.push(0);

        Self {
            tree,
            low: Some(key.to_vec()),
            high: Some(high),
            newest,
            wanted: BTreeSet::from([key.to_vec()]),
        }
    }

    fn holds(&self, key: &[u8]) -> bool {
        self.low.as_deref().is_none_or(|low| key >= low)
            && self.high.as_deref().is_none_or(|high| key < high)
    }

    /// Whether `leaf` is a version of some of the gap's keys.
    fn meets(&self, leaf: &Leaf) -> bool {
        leaf.pointer.txn <= self.newest
            && self
                .low
                .as_deref()
                .is_none_or(|low| leaf.last.as_slice() >= low)
            && self
                .high
                .as_deref()
                .is_none_or(|high| leaf.first.as_slice() < high)
    }
}

/// Closed spans of keys, merged where they overlap.
#[derive(Default)]
struct Spans(BTreeMap<Vec<u8>, Vec<u8>>);

impl Spans {
    fn contains(&self, key: &[u8]) -> bool {
        self.0
            .range::<[u8], _>((Bound::Unbounded, Bound::Included(key)))
            .next_back()
            .is_some_and(|(_, last)| key <= last.as_slice())
    }

    fn add(&mut self, first: &[u8], last: &[u8]) {
        let mut first = first.to_vec();
        let mut last = last.to_vec();

        // A span that starts before this one and reaches into it.
        if let Some((start, end)) = self
            .0
            .range::<[u8], _>((Bound::Unbounded, Bound::Included(first.as_slice())))
            .next_back()
        {
            if *end >= first {
                first.clone_from(start);
                last = last.max(end.clone());
            }
        }

        let inside: Vec<Vec<u8>> = self
            .0
            .range::<[u8], _>((
                Bound::Included(first.as_slice()),
                Bound::Included(last.as_slice()),
            ))
            .map(|(start, _)| start.clone())
            .collect();

        for start in inside {
            if let Some(end) = self.0.remove(&start) {
                last = last.max(end);
            }
        }

        self.0.insert(first, last);
    }
}

struct Salvager<'i> {
    input: &'i Input,
    /// The leaves the scan found, by tree, newest first.
    leaves: HashMap<u64, Vec<Leaf>>,
}

impl Salvager<'_> {
    /// Copies every tree of `record`, or of the catalog the scan found when
    /// there is no record, into `copy`, filling what could not be read from
    /// older leaves. The indexes are left out, to be built again. Returns
    /// the names of the trees copied.
    fn copy(
        &self,
        record: Option<&CommitRecord>,
        copy: &Database,
        report: &mut SalvageReport,
    ) -> Result<Vec<String>> {
        let mut trees = BTreeMap::new();
        let mut catalog = |key: &[u8], value: &[u8]| {
            trees.extend(descriptor(key, value));

            Ok(())
        };
        let gaps = match record {
            Some(record) => {
                let gaps = self.walk(CATALOG_TREE, record.catalog, &mut catalog)?;

                report.pages_unread += gaps.len() as u64;
                gaps
            }
            None => vec![Gap::everything(CATALOG_TREE)],
        };

        for gap in gaps {
            self.fill(gap, &mut catalog)?;
        }

        let mut out = Output {
            db: copy,
            txn: None,
            held: 0,
        };
        let mut names = Vec::new();

        for (name, tree) in trees {
            if name.starts_with(INDEX_PREFIX) {
                continue;
            }

            let mut empty = true;
            let mut visit = |key: &[u8], value: &[u8]| {
                empty = false;
                out.insert(&name, key, value)
            };
            let gaps = self.walk(tree.id, tree.root, &mut visit)?;

            report.pages_unread += gaps.len() as u64;

            for gap in gaps {
                let (taken, lost) = self.fill(gap, &mut visit)?;

                report.entries_recovered += taken;
                report.values_lost += lost;
            }

            // A tree with no entries is copied as one.
            if empty {
                out.create(&name)?;
            }

            names.push(name);
        }

        out.commit()?;

        Ok(names)
    }

    /// Walks tree `tree` from `root`, giving `visit` every entry it reads
    /// whole, in key order, and returns what it could not read.
    fn walk(&self, tree: u64, root: Pointer, visit: &mut Visit<'_>) -> Result<Vec<Gap>> {
        let mut gaps = Vec::new();

        if root.is_null() {
            return Ok(gaps);
        }

        let mut pending: Vec<Pending> = vec![(root, None, None, None)];

        while let Some((pointer, level, low, high)) = pending.pop() {
            let Ok(loaded) = self.input.loader.load(&pointer, tree, level) else {
                gaps.push(Gap {
                    tree,
                    low,
                    high,
                    newest: pointer.txn,
                    wanted: BTreeSet::new(),
                });

                continue;
            };
            let level = loaded.level();
            let node = NodeRef::Loaded(loaded);
            let count = node.count();

            if !node.is_leaf() {
                let NodeRef::Loaded(loaded) = &node else {
                    continue;
                };

                // Last child first, so that the first is read first.
                for index in (0..=count).rev() {
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
                if !self.visit_entry(&node, index, tree, visit)? {
                    gaps.push(Gap::value(tree, node.key(index), pointer.txn));
                }
            }
        }

        Ok(gaps)
    }

    /// Fills `gap` from the leaves the scan found, newest first, giving
    /// `visit` each entry it takes. A key is taken from the newest leaf whose
    /// span covers it, since that version says whether the key still
    /// existed, or from an older one where that version's value cannot be
    /// read. Returns how many entries it took, and how many keys it found no
    /// readable value for.
    fn fill(&self, mut gap: Gap, visit: &mut Visit<'_>) -> Result<(u64, u64)> {
        let mut wanted = std::mem::take(&mut gap.wanted);
        let mut covered = Spans::default();
        let mut taken = 0;
        let leaves = self.leaves.get(&gap.tree).map_or(&[][..], Vec::as_slice);

        for leaf in leaves.iter().filter(|leaf| gap.meets(leaf)) {
            // The scan read it whole, so this fails only if the file does.
            let Ok(loaded) = self.input.loader.load(&leaf.pointer, gap.tree, Some(0)) else {
                continue;
            };
            let node = NodeRef::Loaded(loaded);

            for index in 0..node.count() {
                let key = node.key(index);
                let sought = wanted.contains(key);

                if !gap.holds(key) || (!sought && covered.contains(key)) {
                    continue;
                }

                if self.visit_entry(&node, index, gap.tree, visit)? {
                    taken += 1;

                    if sought {
                        wanted.remove(key);
                    }
                } else if !sought {
                    wanted.insert(key.to_vec());
                }
            }

            covered.add(&leaf.first, &leaf.last);
        }

        Ok((taken, wanted.len() as u64))
    }

    /// Gives `visit` entry `index` of leaf `node` of tree `tree`, and says
    /// whether its value could be read.
    fn visit_entry(
        &self,
        node: &NodeRef<'_>,
        index: usize,
        tree: u64,
        visit: &mut Visit<'_>,
    ) -> Result<bool> {
        let Ok((key, value)) = node.entry(index) else {
            return Ok(false);
        };

        match value {
            StoredRef::Inline(value) => visit(key, value)?,
            StoredRef::Overflow(reference) => {
                match self.input.loader.read_overflow(&reference, tree) {
                    Ok(value) => visit(key, &value)?,
                    Err(_) => return Ok(false),
                }
            }
        }

        Ok(true)
    }
}

/// The tree a catalog entry names, if it names one of the trees a commit
/// copies.
fn descriptor(key: &[u8], value: &[u8]) -> Option<(String, TreeDescriptor)> {
    let name = String::from_utf8(key.to_vec()).ok()?;
    let descriptor = TreeDescriptor::decode(value).ok()?;

    (descriptor.id >= FIRST_USER_TREE).then_some((name, descriptor))
}

/// The write transactions that fill the new file, one after another, each
/// committed once it holds [`COMMIT_BYTES`] of keys and values.
struct Output<'d> {
    db: &'d Database,
    txn: Option<WriteTransaction>,
    held: usize,
}

impl Output<'_> {
    fn txn(&mut self) -> Result<&mut WriteTransaction> {
        let txn = match self.txn.take() {
            Some(txn) => txn,
            None => self.db.begin_write()?,
        };

        Ok(self.txn.insert(txn))
    }

    fn insert(&mut self, tree: &str, key: &[u8], value: &[u8]) -> Result<()> {
        self.txn()?.insert_in(tree, key, value)?;
        self.held += key.len() + value.len();

        if self.held >= COMMIT_BYTES {
            self.commit()?;
        }

        Ok(())
    }

    /// Makes `tree` a tree of the new file, with no entries.
    fn create(&mut self, tree: &str) -> Result<()> {
        let txn = self.txn()?;

        txn.insert_in(tree, b"", b"")?;
        txn.remove_in(tree, b"")?;

        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if let Some(txn) = self.txn.take() {
            txn.commit_deferred()?;
        }

        self.held = 0;

        Ok(())
    }
}

/// Makes the object layer of the new file whole again: records that cannot
/// be read go, every index is built again from the records, and every
/// auto-increment counter lies past its keys. Without a stored schema,
/// nothing could read the objects, so they go too.
fn repair(copy: &Database, names: &[String], report: &mut SalvageReport) -> Result<()> {
    let stored = copy.begin_read()?.get_in(META, SCHEMA_KEY)?;
    let schema = stored
        .as_deref()
        .and_then(|stored| StoredSchema::decode(stored).ok());
    let collections = schema
        .as_ref()
        .map_or(&[][..], |schema| schema.collections.as_slice());
    let known: HashSet<String> = collections
        .iter()
        .map(|collection| String::from(&*records(collection.id)))
        .collect();
    let mut txn = copy.begin_write()?;

    for name in names {
        if name.starts_with(RECORDS_PREFIX) && !known.contains(name) {
            report.objects_dropped += txn.len_in(name)?;
            txn.delete_tree_in(name)?;
        }
    }

    if schema.is_none() {
        txn.delete_tree_in(META)?;
    }

    txn.commit_deferred()?;

    for collection in collections {
        rebuild(copy, collection, report)?;
    }

    Ok(())
}

/// The entries an object gives the indexes of its collection: each index's
/// id, whether it is unique, the entry and its value.
type Entries = Vec<(u64, bool, Vec<u8>, Vec<u8>)>;

/// Builds the indexes of `collection` from its records, dropping the
/// objects that cannot be read or that take a unique index's entry another
/// object has, and moves its counter past its keys.
fn rebuild(copy: &Database, collection: &CollectionDef, report: &mut SalvageReport) -> Result<()> {
    let tree = records(collection.id);
    let mut after: Option<Vec<u8>> = None;
    let mut largest: Option<i64> = None;

    loop {
        let mut txn = copy.begin_write()?;
        let start = after.as_deref().map_or(Bound::Unbounded, Bound::Excluded);
        let batch = txn
            .range_in::<&[u8]>(&tree, &(start, Bound::Unbounded), false)?
            .take(REBUILD_BATCH)
            .collect::<Result<Vec<_>>>()?;
        let max_key_len = txn.max_key_len();

        for (key, bytes) in &batch {
            let kept = match entries_of(collection, key, bytes, max_key_len) {
                Some((entries, id)) if !claimed(&txn, &entries)? => {
                    for (index, _, entry, value) in &entries {
                        txn.insert_in(&index_tree(*index), entry, value)?;
                    }

                    largest = largest.max(id);
                    true
                }
                _ => false,
            };

            if !kept {
                txn.remove_in(&tree, key)?;
                report.objects_dropped += 1;
            }
        }

        let done = batch.len() < REBUILD_BATCH;

        if let Some((key, _)) = batch.into_iter().next_back() {
            after = Some(key);
        }

        if done && collection.auto {
            raise_counter(&mut txn, collection.id, largest)?;
        }

        txn.commit_deferred()?;

        if done {
            return Ok(());
        }
    }
}

/// The index entries of the object whose record is `bytes`, stored under
/// `key`, and its key if it is an integer. None when the record does not
/// decode, is not stored under its own key, or gives an entry too long for
/// the file.
fn entries_of(
    collection: &CollectionDef,
    key: &[u8],
    bytes: &[u8],
    max_key_len: usize,
) -> Option<(Entries, Option<i64>)> {
    let object = codec::object_of(bytes, &collection.fields).ok()?;
    let own = collection
        .key_field()
        .and_then(|field| object.get(&field.name))?;

    if key::encoded(own).ok()? != key {
        return None;
    }

    let mut entries = Vec::new();

    for index in &collection.indexes {
        for (entry, value) in index_entries(index, collection, &object, key).ok()? {
            if entry.len() > max_key_len {
                return None;
            }

            entries.push((index.id, index.unique, entry, value));
        }
    }

    let id = match own {
        Value::Int(id) => Some(*id),
        _ => None,
    };

    Some((entries, id))
}

/// Whether another object has taken one of the unique entries in `entries`.
fn claimed(txn: &WriteTransaction, entries: &Entries) -> Result<bool> {
    for (index, unique, entry, _) in entries {
        if *unique && txn.get_in(&index_tree(*index), entry)?.is_some() {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Moves the auto-increment counter of collection `id` past `largest`, the
/// largest key its objects have, where it is not past it already.
fn raise_counter(txn: &mut WriteTransaction, id: u64, largest: Option<i64>) -> Result<()> {
    let name = counter(id);
    let next = match txn.get_in(META, name.as_bytes())? {
        // A collection that never numbered an object starts at 1.
        None => Some(1),
        Some(bytes) => <[u8; 8]>::try_from(bytes.as_slice())
            .ok()
            .map(u64::from_le_bytes),
    };
    let past = largest
        .and_then(|largest| u64::try_from(largest).ok())
        .map_or(1, |largest| largest.saturating_add(1));

    if next.is_none_or(|next| next < past) {
        txn.insert_in(META, name.as_bytes(), &past.to_le_bytes())?;
    }

    Ok(())
}

/// Counts the trees and entries of the new file into the report.
fn count(copy: &Database, report: &mut SalvageReport) -> Result<()> {
    let read = copy.begin_read()?;
    let names = read.tree_names_in()?;

    report.trees = names.len() as u64;
    report.entries = names
        .iter()
        .map(|name| read.len_in(name))
        .sum::<Result<u64>>()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::testing::Rng;
    use crate::{Collection, Object, Schema, Type};

    /// Every tree of a database by name, each with its entries in order.
    type Trees = BTreeMap<String, Vec<(Vec<u8>, Vec<u8>)>>;

    /// Every tree the database holds, the engine's own included.
    fn everything(db: &Database) -> Trees {
        let read = db.begin_read().unwrap();

        read.tree_names_in()
            .unwrap()
            .into_iter()
            .map(|name| {
                let entries = read
                    .range_in::<&[u8]>(
                        &name,
                        &(Bound::<&[u8]>::Unbounded, Bound::<&[u8]>::Unbounded),
                        false,
                    )
                    .unwrap()
                    .collect::<Result<Vec<_>>>()
                    .unwrap();

                (name, entries)
            })
            .collect()
    }

    fn keyed() -> OpenOptions {
        let mut options = OpenOptions::new();

        options.key([5; 32]);
        options
    }

    /// Plain and encrypted options, in turn.
    fn both() -> [OpenOptions; 2] {
        [OpenOptions::new(), keyed()]
    }

    fn schema() -> Schema {
        Schema::new(1).collection(
            Collection::new("people")
                .field("name", Type::String)
                .field("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email")
                .index("age"),
        )
    }

    /// Flips a byte in the content of page `page`.
    fn damage(path: &Path, page: u64, page_size: u64) {
        let mut bytes = fs::read(path).unwrap();
        let at = usize::try_from(page * page_size).unwrap() + 200;

        bytes[at] ^= 0xFF;
        fs::write(path, bytes).unwrap();
    }

    /// Where the published commit of the closed file at `path` keeps `key`
    /// of tree `tree`: the leaf's page, how many entries the leaf holds, and
    /// the first page of the value's overflow run, if it has one.
    fn located(
        path: &Path,
        options: &OpenOptions,
        tree: &str,
        key: &[u8],
    ) -> (u64, usize, Option<u64>) {
        let input = Input::open(path, options).unwrap();
        let salvager = Salvager {
            input: &input,
            leaves: HashMap::new(),
        };
        let mut trees = BTreeMap::new();

        salvager
            .walk(CATALOG_TREE, input.records[0].catalog, &mut |key, value| {
                trees.extend(descriptor(key, value));

                Ok(())
            })
            .unwrap();

        let descriptor = trees[tree];
        let mut pointer = descriptor.root;

        loop {
            let loaded = input.loader.load(&pointer, descriptor.id, None).unwrap();
            let node = NodeRef::Loaded(Arc::clone(&loaded));

            if node.is_leaf() {
                let overflow = (0..node.count())
                    .find(|&index| node.key(index) == key)
                    .and_then(|index| match node.entry(index).unwrap().1 {
                        StoredRef::Overflow(reference) => Some(reference.first),
                        StoredRef::Inline(_) => None,
                    });

                return (pointer.page, node.count(), overflow);
            }

            let index = (0..node.count())
                .take_while(|&index| node.key(index) <= key)
                .count();

            pointer = loaded.child(index);
        }
    }

    /// The name of the one tree of objects in the file.
    fn records_tree(db: &Database) -> String {
        everything(db)
            .into_keys()
            .find(|name| name.starts_with(RECORDS_PREFIX))
            .unwrap()
    }

    #[test]
    fn a_whole_file_comes_out_as_it_went_in() {
        let dir = tempfile::tempdir().unwrap();
        let mut password = OpenOptions::new();

        password
            .password("correct horse")
            .password_hashing(8 * 1024, 1, 1);

        for (at, mut options) in [OpenOptions::new(), keyed(), password]
            .into_iter()
            .enumerate()
        {
            options.schema(schema());

            let from = dir.path().join(format!("whole-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let db = options.open(&from).unwrap();
            let mut txn = db.begin_write().unwrap();

            for n in 0..500 {
                txn.collection("people")
                    .unwrap()
                    .insert(
                        Object::new()
                            .with("name", format!("person {n}"))
                            .with("email", format!("{n}@example.com"))
                            .with("age", n % 40),
                    )
                    .unwrap();
            }

            txn.insert("large", b"value", &vec![7; 30_000]).unwrap();
            txn.insert("emptied", b"gone", b"").unwrap();
            txn.commit().unwrap();

            let mut txn = db.begin_write().unwrap();

            txn.remove("emptied", b"gone").unwrap();
            txn.collection("people").unwrap().delete(7).unwrap();
            txn.commit().unwrap();

            let before = everything(&db);
            let commit = db.begin_read().unwrap().commit_id();

            drop(db);

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();

            assert!(report.is_whole(), "{report:?}");
            assert_eq!(report.commit_id, Some(commit));
            assert_eq!(report.pages_damaged, 0, "{report:?}");
            assert_eq!(everything(&copy), before);
            assert!(copy.check().unwrap().is_ok(), "{:?}", copy.check().unwrap());
            assert_eq!(report.trees, before.len() as u64);
            assert_eq!(
                report.entries,
                before
                    .values()
                    .map(|entries| entries.len() as u64)
                    .sum::<u64>()
            );
            assert_eq!(report.bytes, fs::metadata(&into).unwrap().len());
        }
    }

    /// A leaf of the last commit, damaged, comes back as the commit before
    /// wrote it, and nothing else changes.
    #[test]
    fn a_damaged_leaf_comes_back_from_its_older_version() {
        let dir = tempfile::tempdir().unwrap();

        for (at, options) in both().into_iter().enumerate() {
            let from = dir.path().join(format!("leaf-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let key = |n: u32| format!("key {n:05}").into_bytes();
            let db = options.open(&from).unwrap();

            for version in ["old", "new"] {
                let mut txn = db.begin_write().unwrap();

                for n in 0..2_000 {
                    txn.insert("t", &key(n), format!("{version} {n:05}").as_bytes())
                        .unwrap();
                }

                txn.commit().unwrap();
            }

            drop(db);

            let (page, held, _) = located(&from, &options, "t", &key(1_000));

            damage(&from, page, 4096);

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();
            let read = copy.begin_read().unwrap();
            let old: Vec<Vec<u8>> = read
                .iter("t")
                .unwrap()
                .map(Result::unwrap)
                .filter(|(_, value)| value.starts_with(b"old"))
                .map(|(key, _)| key)
                .collect();

            assert_eq!(read.len("t").unwrap(), 2_000);
            assert!(old.contains(&key(1_000)), "{report:?}");
            assert_eq!(old.len(), held, "{report:?}");
            assert_eq!(report.pages_damaged, 1, "{report:?}");
            assert_eq!(report.pages_unread, 1, "{report:?}");
            assert_eq!(report.entries_recovered, held as u64, "{report:?}");
            assert_eq!(report.values_lost, 0, "{report:?}");
            assert!(!report.is_whole());
            assert!(copy.check().unwrap().is_ok(), "{:?}", copy.check().unwrap());
        }
    }

    /// A key that a newer version of a leaf no longer holds stays removed,
    /// though an older version still has it.
    #[test]
    fn a_key_a_newer_version_removed_stays_removed() {
        let dir = tempfile::tempdir().unwrap();

        for (at, options) in both().into_iter().enumerate() {
            let from = dir.path().join(format!("removed-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let key = |n: u32| format!("key {n:05}").into_bytes();
            let db = options.open(&from).unwrap();
            let write = |version: &str, removed: Option<u32>| {
                let mut txn = db.begin_write().unwrap();

                for n in 0..2_000 {
                    if Some(n) == removed {
                        txn.remove("t", &key(n)).unwrap();
                    } else if version == "first" || n != 1_000 {
                        txn.insert("t", &key(n), format!("{version} {n:05}").as_bytes())
                            .unwrap();
                    }
                }

                txn.commit().unwrap();
            };

            write("first", None);

            // Keeps the first version's pages from being written over.
            let reader = db.begin_read().unwrap();

            write("second", Some(1_000));
            write("third", None);
            drop(reader);
            drop(db);

            let (page, held, _) = located(&from, &options, "t", &key(1_000));

            damage(&from, page, 4096);

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();
            let read = copy.begin_read().unwrap();

            assert_eq!(read.get("t", &key(1_000)).unwrap(), None, "{report:?}");
            assert_eq!(read.len("t").unwrap(), 1_999);
            assert_eq!(report.entries_recovered, held as u64, "{report:?}");
            assert!(
                read.iter("t")
                    .unwrap()
                    .map(Result::unwrap)
                    .all(|(_, value)| !value.starts_with(b"first"))
            );
        }
    }

    /// A commit whose record fails its check never happened, so its pages
    /// fill nothing: the commit before it is where salvage starts, and its
    /// damage is filled from older pages still.
    #[test]
    fn a_commit_whose_record_is_lost_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();

        for (at, options) in both().into_iter().enumerate() {
            let from = dir.path().join(format!("record-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let key = |n: u32| format!("key {n:05}").into_bytes();
            let db = options.open(&from).unwrap();
            let write = |version: &str| {
                let mut txn = db.begin_write().unwrap();

                for n in 0..2_000 {
                    txn.insert("t", &key(n), format!("{version} {n:05}").as_bytes())
                        .unwrap();
                }

                txn.commit().unwrap();
            };

            write("first");

            // Keeps the first version's pages from being written over.
            let reader = db.begin_read().unwrap();

            write("second");
            write("third");
            drop(reader);
            drop(db);

            // The newest record goes, and then a leaf of the one before it.
            let mut bytes = fs::read(&from).unwrap();
            let newest = (0..SLOT_COUNT)
                .max_by_key(|&slot| {
                    CommitRecord::decode(slot, &bytes[slot_offset(slot)..])
                        .unwrap()
                        .map_or(0, |record| record.txn)
                })
                .unwrap();

            bytes[slot_offset(newest) + 8] ^= 0xFF;
            fs::write(&from, bytes).unwrap();

            let (page, held, _) = located(&from, &options, "t", &key(1_000));

            damage(&from, page, 4096);

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();
            let read = copy.begin_read().unwrap();
            let versions: BTreeMap<String, usize> = read
                .iter("t")
                .unwrap()
                .map(Result::unwrap)
                .fold(BTreeMap::new(), |mut versions, (_, value)| {
                    let version = String::from_utf8(value).unwrap();

                    *versions
                        .entry(version.split(' ').next().unwrap().to_owned())
                        .or_default() += 1;
                    versions
                });

            assert_eq!(
                versions,
                BTreeMap::from([
                    ("first".to_owned(), held),
                    ("second".to_owned(), 2_000 - held)
                ]),
                "{report:?}"
            );
        }
    }

    /// A value whose overflow run is damaged comes back as the commit before
    /// wrote it, and one no older commit wrote is lost and counted.
    #[test]
    fn a_damaged_value_comes_back_from_an_older_version_or_is_counted_lost() {
        let dir = tempfile::tempdir().unwrap();

        for (at, options) in both().into_iter().enumerate() {
            let from = dir.path().join(format!("value-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let db = options.open(&from).unwrap();
            let mut txn = db.begin_write().unwrap();

            txn.insert("t", b"document", &vec![3; 20_000]).unwrap();
            txn.insert("t", b"photo", &vec![1; 20_000]).unwrap();
            txn.insert("t", b"small", b"stays").unwrap();
            txn.commit().unwrap();

            let mut txn = db.begin_write().unwrap();

            txn.insert("t", b"photo", &vec![2; 20_000]).unwrap();
            txn.commit().unwrap();
            drop(db);

            for key in [&b"document"[..], b"photo"] {
                let (_, _, overflow) = located(&from, &options, "t", key);

                damage(&from, overflow.unwrap() + 1, 4096);
            }

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();
            let read = copy.begin_read().unwrap();

            assert_eq!(read.get("t", b"photo").unwrap(), Some(vec![1; 20_000]));
            assert_eq!(read.get("t", b"document").unwrap(), None);
            assert_eq!(read.get("t", b"small").unwrap(), Some(b"stays".to_vec()));
            assert_eq!(report.pages_unread, 2, "{report:?}");
            assert_eq!(report.entries_recovered, 1, "{report:?}");
            assert_eq!(report.values_lost, 1, "{report:?}");
            assert!(copy.check().unwrap().is_ok());
        }
    }

    /// With every commit record gone, the trees come from the newest
    /// catalog leaf the scan finds, and hold what the last commit held.
    #[test]
    fn without_a_commit_record_the_trees_come_from_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("records.darudb");
        let into = dir.path().join("salvaged.darudb");
        let db = OpenOptions::new().open(&from).unwrap();

        for round in 0..3u8 {
            let mut txn = db.begin_write().unwrap();

            for n in 0..800u32 {
                if n % 3 == u32::from(round) {
                    txn.remove("a", &n.to_be_bytes()).unwrap();
                } else {
                    txn.insert("a", &n.to_be_bytes(), &[round; 40]).unwrap();
                }
            }

            txn.insert("b", &[round], &[round]).unwrap();
            txn.commit().unwrap();
        }

        let before = everything(&db);

        drop(db);

        // Every slot, and so every record, gone.
        let mut bytes = fs::read(&from).unwrap();

        bytes[512..HEADER_LEN].fill(0xA5);
        fs::write(&from, bytes).unwrap();

        let report = OpenOptions::new().salvage(&from, &into).unwrap();
        let copy = OpenOptions::new().open(&into).unwrap();

        assert_eq!(report.commit_id, None);
        assert!(!report.is_whole());
        assert_eq!(everything(&copy), before);
        assert!(copy.check().unwrap().is_ok());

        // An encrypted file keeps its key block in the records.
        let from = dir.path().join("encrypted.darudb");
        let db = keyed().open(&from).unwrap();

        let mut txn = db.begin_write().unwrap();

        txn.insert("a", b"k", b"v").unwrap();
        txn.commit().unwrap();
        drop(db);

        let mut bytes = fs::read(&from).unwrap();

        bytes[512..HEADER_LEN].fill(0xA5);
        fs::write(&from, bytes).unwrap();

        assert_eq!(
            keyed()
                .salvage(&from, dir.path().join("none.darudb"))
                .unwrap_err()
                .code(),
            "CORRUPTED"
        );
    }

    /// A plain file whose static fields are damaged has its page size found
    /// from its pages; an encrypted one cannot do without them.
    #[test]
    fn a_damaged_header_is_found_again_in_a_plain_file() {
        let dir = tempfile::tempdir().unwrap();

        for (at, mut options) in both().into_iter().enumerate() {
            options.page_size(16_384);

            let from = dir.path().join(format!("header-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let db = options.open(&from).unwrap();
            let mut txn = db.begin_write().unwrap();

            for n in 0..3_000u32 {
                txn.insert("t", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
            }

            txn.commit().unwrap();

            let before = everything(&db);

            drop(db);

            // The file id, which the static check covers.
            let mut bytes = fs::read(&from).unwrap();

            bytes[16..32].fill(0);
            fs::write(&from, bytes).unwrap();

            match options.salvage(&from, &into) {
                Ok(report) => {
                    assert_eq!(at, 0, "{report:?}");

                    let copy = options.open(&into).unwrap();

                    assert!(report.is_whole(), "{report:?}");
                    assert_eq!(copy.page_size(), 16_384);
                    assert_eq!(everything(&copy), before);
                    assert!(copy.check().unwrap().is_ok());
                }
                Err(error) => {
                    assert_eq!(at, 1, "{error}");
                    assert_eq!(error.code(), "CORRUPTED");
                }
            }
        }
    }

    /// Objects come back with their indexes built again: a record from an
    /// older version whose unique value another object took since is
    /// dropped, and the counter lies past every key.
    #[test]
    fn objects_come_back_with_indexes_that_match_them() {
        let dir = tempfile::tempdir().unwrap();

        for (at, mut options) in both().into_iter().enumerate() {
            options.schema(schema());

            let from = dir.path().join(format!("objects-{at}.darudb"));
            let into = dir.path().join(format!("salvaged-{at}.darudb"));
            let db = options.open(&from).unwrap();
            let mut txn = db.begin_write().unwrap();

            for n in 1..=600 {
                txn.collection("people")
                    .unwrap()
                    .insert(
                        Object::new()
                            .with("name", format!("person {n}"))
                            .with("email", format!("{n}@example.com"))
                            .with("age", n % 50),
                    )
                    .unwrap();
            }

            txn.commit().unwrap();

            // Person 5 gives up an address, and person 590 takes it.
            let mut txn = db.begin_write().unwrap();

            {
                let mut people = txn.collection("people").unwrap();

                people
                    .update(5, Object::new().with("email", "new@example.com"))
                    .unwrap();
                people
                    .update(590, Object::new().with("email", "5@example.com"))
                    .unwrap();
            }

            txn.commit().unwrap();

            let tree = records_tree(&db);

            drop(db);

            let key = key::encoded(&Value::Int(5)).unwrap();
            let (page, _, _) = located(&from, &options, &tree, &key);

            damage(&from, page, 4096);

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();
            let people: Vec<Object> = copy
                .begin_read()
                .unwrap()
                .collection("people")
                .unwrap()
                .iter()
                .unwrap()
                .map(Result::unwrap)
                .collect();
            let taken = people
                .iter()
                .filter(|person| person.get("email") == Some(&Value::from("5@example.com")))
                .count();

            assert!(copy.check().unwrap().is_ok(), "{:?}", copy.check().unwrap());
            assert_eq!(report.objects_dropped, 1, "{report:?}");
            assert_eq!(people.len(), 599, "{report:?}");
            assert_eq!(taken, 1);

            // The counter lies past every key.
            let mut txn = copy.begin_write().unwrap();
            let id = txn
                .collection("people")
                .unwrap()
                .insert(
                    Object::new()
                        .with("name", "late")
                        .with("email", "late@example.com"),
                )
                .unwrap();

            assert_eq!(id, Value::Int(601));
        }
    }

    /// The object layer's own records taken from an older version bring
    /// an older auto-increment counter, which salvage moves past every key.
    #[test]
    fn a_counter_from_an_older_version_moves_past_every_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        let from = dir.path().join("counter.darudb");
        let into = dir.path().join("salvaged.darudb");
        let db = options.open(&from).unwrap();
        let insert = |range: std::ops::RangeInclusive<i64>| {
            let mut txn = db.begin_write().unwrap();

            for n in range {
                txn.collection("people")
                    .unwrap()
                    .insert(
                        Object::new()
                            .with("name", format!("person {n}"))
                            .with("email", format!("{n}@example.com")),
                    )
                    .unwrap();
            }

            txn.commit().unwrap();
        };

        insert(1..=300);

        let reader = db.begin_read().unwrap();

        insert(301..=600);
        drop(reader);
        drop(db);

        let (page, _, _) = located(&from, &options, META, SCHEMA_KEY);

        damage(&from, page, 4096);

        let report = options.salvage(&from, &into).unwrap();
        let copy = options.open(&into).unwrap();

        assert!(report.entries_recovered > 0, "{report:?}");
        assert!(copy.check().unwrap().is_ok(), "{:?}", copy.check().unwrap());

        let mut txn = copy.begin_write().unwrap();
        let id = txn
            .collection("people")
            .unwrap()
            .insert(
                Object::new()
                    .with("name", "late")
                    .with("email", "late@example.com"),
            )
            .unwrap();

        assert_eq!(id, Value::Int(601));
    }

    #[test]
    fn spans_merge_where_they_overlap() {
        let mut spans = Spans::default();

        spans.add(b"d", b"f");
        spans.add(b"m", b"p");
        spans.add(b"a", b"b");

        assert!(spans.contains(b"e") && spans.contains(b"p") && spans.contains(b"a"));
        assert!(!spans.contains(b"c") && !spans.contains(b"g") && !spans.contains(b"q"));

        // Bridging two spans and reaching past the second.
        spans.add(b"e", b"n");

        assert!(spans.contains(b"h") && spans.contains(b"o"));
        assert_eq!(spans.0.len(), 2, "{:?}", spans.0);

        // Keys lie between "b" and "c", so these two stay apart.
        spans.add(b"c", b"z");

        assert_eq!(spans.0.len(), 2, "{:?}", spans.0);
        assert!(spans.contains(b"c") && spans.contains(b"y") && !spans.contains(b"ba"));
    }

    #[test]
    fn a_file_in_use_or_a_path_taken_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("in-use.darudb");
        let taken = dir.path().join("taken.darudb");
        let db = OpenOptions::new().open(&from).unwrap();

        let mut txn = db.begin_write().unwrap();

        txn.insert("t", b"k", b"v").unwrap();
        txn.commit().unwrap();

        let into = dir.path().join("salvaged.darudb");

        assert_eq!(
            OpenOptions::new().salvage(&from, &into).unwrap_err().code(),
            "BUSY"
        );

        // The handle kept its locks: it still writes.
        let mut txn = db.begin_write().unwrap();

        txn.insert("t", b"k2", b"v2").unwrap();
        txn.commit().unwrap();
        drop(db);

        fs::write(&taken, b"someone else's").unwrap();

        assert_eq!(
            OpenOptions::new()
                .salvage(&from, &taken)
                .unwrap_err()
                .code(),
            "INVALID_ARGUMENT"
        );
        assert_eq!(fs::read(&taken).unwrap(), b"someone else's");
        assert_eq!(
            keyed().salvage(&from, &into).unwrap_err().code(),
            "INVALID_ARGUMENT"
        );

        let noise = dir.path().join("noise.bin");

        fs::write(&noise, Rng::new(1).bytes(40_000)).unwrap();
        assert_eq!(
            OpenOptions::new()
                .salvage(&noise, &into)
                .unwrap_err()
                .code(),
            "NOT_A_DATABASE"
        );

        let names: Vec<PathBuf> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        assert_eq!(names.len(), 3, "{names:?}");
    }

    /// Opening a file that salvage holds fails at once, and other files
    /// open meanwhile; once salvage lets go, the file opens again.
    #[test]
    fn opening_a_file_under_salvage_fails_with_busy() {
        use std::sync::mpsc;

        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("held.darudb");
        let into = dir.path().join("salvaged.darudb");
        let db = OpenOptions::new().open(&from).unwrap();

        let mut txn = db.begin_write().unwrap();

        txn.insert("t", b"k", b"v").unwrap();
        txn.commit().unwrap();
        drop(db);

        let (paused, on_pause) = mpsc::channel();
        let (resume, on_resume) = mpsc::channel();

        *crate::testing::PAUSE_IN_SALVAGE.lock().unwrap() = Some((from.clone(), paused, on_resume));

        let salvage = {
            let (from, into) = (from.clone(), into.clone());

            std::thread::spawn(move || OpenOptions::new().salvage(from, into))
        };

        on_pause.recv().unwrap();

        assert_eq!(OpenOptions::new().open(&from).unwrap_err().code(), "BUSY");
        assert_eq!(
            OpenOptions::new()
                .salvage(&from, dir.path().join("second.darudb"))
                .unwrap_err()
                .code(),
            "BUSY"
        );
        OpenOptions::new()
            .open(dir.path().join("other.darudb"))
            .unwrap();

        resume.send(()).unwrap();

        assert!(salvage.join().unwrap().unwrap().is_whole());

        let db = OpenOptions::new().open(&from).unwrap();

        assert_eq!(
            db.begin_read().unwrap().get("t", b"k").unwrap(),
            Some(b"v".to_vec())
        );
    }

    /// Random trees written over several commits, then damaged at random:
    /// salvage always gives a file that passes the check, and every value
    /// in it is one its key held after some commit.
    #[test]
    fn random_damage_never_invents_a_value() {
        let dir = tempfile::tempdir().unwrap();

        for seed in 0..8 {
            let mut rng = Rng::new(seed);
            let options = if seed % 2 == 0 {
                OpenOptions::new()
            } else {
                keyed()
            };
            let from = dir.path().join(format!("random-{seed}.darudb"));
            let into = dir.path().join(format!("salvaged-{seed}.darudb"));
            let db = options.open(&from).unwrap();
            let mut held: HashSet<(String, Vec<u8>, Vec<u8>)> = HashSet::new();

            for _ in 0..8 {
                let mut txn = db.begin_write().unwrap();

                for _ in 0..300 {
                    let tree = ["a", "b", "c"][rng.index(3)].to_owned();
                    let key_len = 1 + rng.index(4);
                    let key = rng.bytes(key_len);

                    if rng.below(4) == 0 {
                        txn.remove(&tree, &key).unwrap();

                        continue;
                    }

                    let len = if rng.below(20) == 0 {
                        rng.index(3 * 4096)
                    } else {
                        rng.index(100)
                    };
                    let value = rng.bytes(len);

                    txn.insert(&tree, &key, &value).unwrap();
                    held.insert((tree, key, value));
                }

                txn.commit().unwrap();
            }

            drop(db);

            let pages = fs::metadata(&from).unwrap().len() / 4096;

            for _ in 0..1 + rng.index(4) {
                damage(&from, 1 + rng.below(pages - 1), 4096);
            }

            let report = options.salvage(&from, &into).unwrap();
            let copy = options.open(&into).unwrap();
            let read = copy.begin_read().unwrap();

            assert!(
                copy.check().unwrap().is_ok(),
                "seed {seed}: {:?}",
                copy.check().unwrap()
            );

            for tree in read.tree_names().unwrap() {
                for entry in read.iter(&tree).unwrap() {
                    let (key, value) = entry.unwrap();

                    assert!(
                        held.contains(&(tree.clone(), key.clone(), value)),
                        "seed {seed}: a value of {key:?} in {tree:?} that no commit wrote: {report:?}"
                    );
                }
            }
        }
    }
}
