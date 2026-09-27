//! Free space during one write transaction.
//!
//! The free tree says which pages the commit a transaction starts from does
//! not use. The transaction starts from its runs in memory, adds the retained
//! groups it may reclaim, and hands pages out from there, lowest first. A page
//! it releases goes back to the free pages if the transaction allocated it
//! itself, and into this commit's retained group if it was committed: a reader
//! or a recovery may still need it. A committed page written after the durable
//! commit goes into the group's young part, which the durable commit does not
//! reach and a later transaction may reclaim before the unsynced window ends.
//! `design/commits-and-recovery.md` is the specification.
//!
//! Every change to the free runs is noted with what the free tree held for
//! that run before, so the commit rewrites only the runs that changed, and an
//! aborted transaction can give the runs it started from back unchanged.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::sync::Arc;

use crate::btree::Store;
use crate::error::Result;
use crate::format::Check;
use crate::storage::Pager;

/// The file never reaches this many bytes: the lock bytes start there.
const LOCK_BASE: u64 = 1 << 62;

/// The young part of one retained group: the pages its commit released that
/// were written after the durable commit, which the writer may reclaim before
/// the unsynced window ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct YoungPart {
    /// The sequence number of its first entry in the retained tree. Its
    /// entries are the group's last.
    pub(crate) first: u32,
    /// How many entries it takes.
    pub(crate) entries: u32,
    pub(crate) runs: Vec<(u64, u32)>,
}

/// The young parts of the retained groups of one commit, by group.
pub(crate) type YoungParts = BTreeMap<u64, YoungPart>;

/// The free space of one write transaction.
#[derive(Debug)]
pub(crate) struct Space {
    pager: Arc<Pager>,
    txn: u64,
    /// Runs of free pages: first page and length. Runs never overlap or touch.
    free: BTreeMap<u64, u64>,
    /// For every run start changed since the free tree last matched `free`,
    /// what the tree holds there: a length, or `None` for no run.
    recorded: BTreeMap<u64, Option<u64>>,
    /// Set once the free tree has been changed to match: `recorded` no
    /// longer leads back to the runs the transaction started from.
    diverged: bool,
    page_count: u64,
    max_page_count: u64,
    /// Pages this transaction allocated.
    fresh: HashSet<u64>,
    /// Pages written after this id are young: the durable commit does not
    /// reach them.
    young_after: u64,
    /// Committed pages this transaction stopped using that the durable
    /// commit may reach: its retained group, but for the young part.
    retired: Vec<u64>,
    /// Committed pages this transaction stopped using that were written after
    /// the durable commit, and pages it set aside while settling, which no
    /// commit reaches: the young part of its retained group.
    retired_young: Vec<u64>,
    /// Pages this transaction allocated and released while settling, to be
    /// handed out again first; see [`Space::settle`].
    set_aside: Vec<u64>,
    /// Counts every change to the free pages, the page count and the retained
    /// group, so the commit can tell when the allocator trees stop changing.
    changes: u64,
    /// Set while the commit settles the allocator trees.
    settling: bool,
}

impl Space {
    /// The space of a transaction `txn` whose base commit has `page_count`
    /// pages and the given free runs, and whose durable commit is `durable`.
    pub(crate) fn new(
        pager: Arc<Pager>,
        txn: u64,
        durable: u64,
        page_count: u64,
        free: BTreeMap<u64, u64>,
    ) -> Self {
        let max_page_count = LOCK_BASE / pager.page_size() as u64;

        Self {
            pager,
            txn,
            free,
            recorded: BTreeMap::new(),
            diverged: false,
            page_count,
            max_page_count,
            fresh: HashSet::new(),
            young_after: durable,
            retired: Vec::new(),
            retired_young: Vec::new(),
            set_aside: Vec::new(),
            changes: 0,
            settling: false,
        }
    }

    /// Switches to how the commit settles the allocator trees: from here on,
    /// a page this transaction allocated and then releases is set aside
    /// instead of becoming free again, and allocations take set-aside pages
    /// before free ones.
    ///
    /// Neither changes the free pages, so a tree that gives a page back and
    /// needs one again in the same round, emptied and refilled or merged and
    /// split, leaves the free tree as it was. Were the page freed instead, the
    /// free tree would chase itself: it would record the page, then take it
    /// again, one round after another.
    pub(crate) fn settle(&mut self) {
        self.settling = true;
    }

    /// Moves the set-aside pages into the young part of the retained group,
    /// once a round has changed nothing else, and says whether there were
    /// any. Nothing reaches them, and a later commit reclaims them like any
    /// other young page.
    pub(crate) fn retire_set_aside(&mut self) -> bool {
        if self.set_aside.is_empty() {
            return false;
        }

        for page in self.set_aside.drain(..) {
            self.fresh.remove(&page);
            self.retired_young.push(page);
        }

        self.changes += 1;

        true
    }

    /// Sets the run starting at `start` to `len` pages, or removes it, and
    /// notes what the free tree holds there.
    fn set_run(&mut self, start: u64, len: Option<u64>) {
        let before = match len {
            Some(len) => self.free.insert(start, len),
            None => self.free.remove(&start),
        };

        self.recorded.entry(start).or_insert(before);
    }

    /// Makes a run of pages free, merging it with the runs it touches.
    pub(crate) fn add_free(&mut self, start: u64, len: u64) {
        let mut start = start;
        let mut len = len;

        if let Some((&before, &before_len)) = self.free.range(..start).next_back() {
            if before + before_len == start {
                self.set_run(before, None);
                start = before;
                len += before_len;
            }
        }

        if let Some(&after_len) = self.free.get(&(start + len)) {
            self.set_run(start + len, None);
            len += after_len;
        }

        self.set_run(start, Some(len));
        self.changes += 1;
    }

    /// Makes a run of pages read from a retained group free, after checking
    /// that it lies inside the file and is not free already.
    pub(crate) fn add_free_checked(&mut self, start: u64, len: u64) -> Result<(), &'static str> {
        let end = start
            .checked_add(len)
            .ok_or("a run of pages past any file")?;

        if start == 0 || len == 0 || end > self.page_count {
            return Err("a run of pages lies outside the file");
        }

        let overlaps_before = self
            .free
            .range(..end)
            .next_back()
            .is_some_and(|(&other, &other_len)| other + other_len > start);

        if overlaps_before {
            return Err("a page is both free and retained");
        }

        self.add_free(start, len);

        Ok(())
    }

    /// Gives the free pages at the end of the file back to the file system.
    pub(crate) fn trim_tail(&mut self) {
        while let Some((&start, &len)) = self.free.last_key_value() {
            if start + len != self.page_count {
                break;
            }

            self.set_run(start, None);
            self.page_count = start;
            self.changes += 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn free(&self) -> &BTreeMap<u64, u64> {
        &self.free
    }

    /// The runs whose entries in the free tree have to change for the tree to
    /// match the free pages: each start with its new length, or `None` to
    /// remove it. From here on the caller is taken to make those changes.
    pub(crate) fn free_tree_changes(&mut self) -> Vec<(u64, Option<u64>)> {
        self.diverged = true;

        std::mem::take(&mut self.recorded)
            .into_iter()
            .filter_map(|(start, held)| {
                let now = self.free.get(&start).copied();

                (now != held).then_some((start, now))
            })
            .collect()
    }

    /// The free runs, once the free tree matches them: what the next
    /// transaction starts from if this one commits.
    pub(crate) fn take_free(&mut self) -> BTreeMap<u64, u64> {
        self.diverged = true;

        std::mem::take(&mut self.free)
    }

    /// The free runs the transaction started from, if they can still be told
    /// apart from its changes: what the next transaction starts from if this
    /// one is aborted.
    pub(crate) fn take_initial_free(&mut self) -> Option<BTreeMap<u64, u64>> {
        if self.diverged {
            return None;
        }

        self.diverged = true;

        for (start, held) in std::mem::take(&mut self.recorded) {
            match held {
                Some(len) => self.free.insert(start, len),
                None => self.free.remove(&start),
            };
        }

        Some(std::mem::take(&mut self.free))
    }

    /// The pages this transaction allocated and still holds.
    pub(crate) fn fresh(&self) -> &HashSet<u64> {
        &self.fresh
    }

    pub(crate) fn page_count(&self) -> u64 {
        self.page_count
    }

    pub(crate) fn changes(&self) -> u64 {
        self.changes
    }

    /// This commit's retained group but for its young part, as runs of
    /// consecutive pages.
    pub(crate) fn retired_runs(&self) -> Vec<(u64, u32)> {
        runs_of(&self.retired)
    }

    /// The young part of this commit's retained group, as runs of
    /// consecutive pages.
    pub(crate) fn young_runs(&self) -> Vec<(u64, u32)> {
        runs_of(&self.retired_young)
    }

    fn extend(&mut self, pages: u64) -> Result<u64> {
        let first = self.page_count;

        if first + pages > self.max_page_count {
            return Err(self.pager.io_error(io::Error::other(
                "the database has reached the largest size a file may have",
            )));
        }

        self.page_count += pages;

        Ok(first)
    }
}

impl Store for Space {
    fn txn(&self) -> u64 {
        self.txn
    }

    fn young_after(&self) -> u64 {
        self.young_after
    }

    fn allocate(&mut self) -> Result<u64> {
        // Still in `fresh`, and taking it changes nothing the trees record.
        if let Some(page) = self.set_aside.pop() {
            return Ok(page);
        }

        let page = match self.free.first_key_value() {
            Some((&start, &len)) => {
                self.set_run(start, None);

                if len > 1 {
                    self.set_run(start + 1, Some(len - 1));
                }

                start
            }
            None => self.extend(1)?,
        };

        self.fresh.insert(page);
        self.changes += 1;

        Ok(page)
    }

    fn allocate_run(&mut self, pages: u64) -> Result<u64> {
        let found = self
            .free
            .iter()
            .find(|(_, len)| **len >= pages)
            .map(|(start, len)| (*start, *len));
        let first = match found {
            Some((start, len)) => {
                self.set_run(start, None);

                if len > pages {
                    self.set_run(start + pages, Some(len - pages));
                }

                start
            }
            None => self.extend(pages)?,
        };

        self.fresh.extend(first..first + pages);
        self.changes += 1;

        Ok(first)
    }

    fn release(&mut self, page: u64, written: u64) {
        if !self.fresh.contains(&page) {
            debug_assert!(
                !self.retired.contains(&page) && !self.retired_young.contains(&page),
                "page {page} released twice"
            );

            // A committed page: a reader or a recovery may still need it.
            if written > self.young_after {
                self.retired_young.push(page);
            } else {
                self.retired.push(page);
            }

            self.changes += 1;
        } else if self.settling {
            debug_assert!(
                !self.set_aside.contains(&page),
                "page {page} released twice"
            );

            self.set_aside.push(page);
        } else {
            self.fresh.remove(&page);
            self.add_free(page, 1);
        }
    }

    fn write_run(&mut self, first: u64, bytes: &mut [u8]) -> Result<Vec<Check>> {
        self.pager.write_run(first, bytes)
    }

    fn run_pages(&self) -> usize {
        self.pager.run_pages()
    }
}

/// `pages`, in any order, as runs of consecutive pages.
fn runs_of(pages: &[u64]) -> Vec<(u64, u32)> {
    let mut pages = pages.to_vec();
    let mut runs: Vec<(u64, u32)> = Vec::new();

    pages.sort_unstable();

    for page in pages {
        match runs.last_mut() {
            Some((start, len)) if *start + u64::from(*len) == page && *len < u32::MAX => {
                *len += 1;
            }
            _ => runs.push((page, 1)),
        }
    }

    runs
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::storage::sim::SimDisk;

    fn space(page_count: u64, free: &[(u64, u64)]) -> Space {
        let pager = Arc::new(Pager::new(
            Arc::new(SimDisk::default()),
            4096,
            PathBuf::from("test.darudb"),
            None,
        ));

        // Transaction 4, whose durable commit is 2.
        Space::new(pager, 4, 2, page_count, free.iter().copied().collect())
    }

    #[test]
    fn pages_come_from_the_front_before_the_file_grows() {
        let mut space = space(10, &[(3, 2), (7, 1)]);

        assert_eq!(space.allocate().unwrap(), 3);
        assert_eq!(space.allocate().unwrap(), 4);
        assert_eq!(space.allocate().unwrap(), 7);
        assert_eq!(space.allocate().unwrap(), 10);
        assert_eq!(space.page_count(), 11);
    }

    #[test]
    fn a_run_takes_the_first_gap_long_enough() {
        let mut space = space(20, &[(2, 1), (5, 4)]);

        assert_eq!(space.allocate_run(3).unwrap(), 5);
        assert_eq!(space.free(), &[(2, 1), (8, 1)].into_iter().collect());
        assert_eq!(
            space.allocate_run(5).unwrap(),
            20,
            "nothing fits: the file grows"
        );
    }

    #[test]
    fn a_page_of_this_transaction_is_free_again_and_a_committed_one_is_retained() {
        let mut space = space(10, &[]);
        let fresh = space.allocate().unwrap();

        space.release(fresh, 4);
        space.release(4, 1);

        assert_eq!(space.free(), &[(fresh, 1)].into_iter().collect());
        assert_eq!(space.retired_runs(), [(4, 1)]);
        assert_eq!(space.young_runs(), []);
    }

    #[test]
    fn a_committed_page_written_after_the_durable_commit_is_young() {
        let mut space = space(10, &[]);

        space.release(4, 1);
        space.release(5, 2);
        space.release(6, 3);
        space.release(7, 3);

        assert_eq!(space.retired_runs(), [(4, 2)]);
        assert_eq!(space.young_runs(), [(6, 2)]);
    }

    #[test]
    fn released_pages_merge_into_runs_and_the_free_tail_is_trimmed() {
        let mut space = space(10, &[(6, 1), (8, 2)]);

        space.add_free(7, 1);

        assert_eq!(space.free(), &[(6, 4)].into_iter().collect());

        space.trim_tail();

        assert_eq!(space.page_count(), 6);
        assert!(space.free().is_empty());
    }

    #[test]
    fn while_settling_a_released_page_is_reused_before_the_free_pages_change() {
        let mut space = space(10, &[(5, 3)]);
        let page = space.allocate().unwrap();

        space.settle();

        let before = space.changes();

        // A tree emptied and refilled in one round gets its own page back.
        space.release(page, 4);

        assert_eq!(space.allocate().unwrap(), page);
        assert_eq!(space.changes(), before);
        assert_eq!(space.free(), &[(6, 2)].into_iter().collect());

        // What is left over when the round ends joins the retained group, as
        // a young page: no commit reaches it.
        space.release(page, 4);

        assert!(space.retire_set_aside());
        assert!(!space.retire_set_aside());
        assert_eq!(space.retired_runs(), []);
        assert_eq!(space.young_runs(), [(page, 1)]);
        assert_eq!(space.free(), &[(6, 2)].into_iter().collect());
    }

    #[test]
    fn the_free_tree_changes_are_the_runs_that_differ_from_it() {
        let mut space = space(20, &[(3, 2), (10, 4)]);

        space.allocate().unwrap();
        space.add_free(8, 1);
        space.add_free(5, 1);

        // Page 3 went; 5 joined the run at 4; 8 is new; 10 to 13 are as they were.
        assert_eq!(
            space.free_tree_changes(),
            [(3, None), (4, Some(2)), (8, Some(1))]
        );
        assert_eq!(space.free_tree_changes(), []);
    }

    #[test]
    fn an_aborted_transaction_gives_back_the_runs_it_started_from() {
        let mut aborted = space(20, &[(3, 2), (10, 4)]);

        aborted.allocate().unwrap();
        aborted.allocate_run(3).unwrap();
        aborted.add_free(1, 1);
        aborted.trim_tail();

        assert_eq!(
            aborted.take_initial_free(),
            Some([(3, 2), (10, 4)].into_iter().collect())
        );

        let mut committed = space(20, &[(3, 2)]);

        committed.free_tree_changes();

        assert_eq!(committed.take_initial_free(), None);
    }

    #[test]
    fn retired_pages_are_grouped_into_runs() {
        let mut space = space(20, &[]);

        for page in [9, 3, 4, 5, 12, 10] {
            space.release(page, 1);
        }

        assert_eq!(space.retired_runs(), [(3, 3), (9, 2), (12, 1)]);
    }
}
