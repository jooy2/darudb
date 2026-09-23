//! Free space during one write transaction.
//!
//! The free tree says which pages the commit a transaction starts from does
//! not use. The transaction loads it into memory, adds the retained groups it
//! may reclaim, and hands pages out from there, lowest first. A page it
//! releases goes back to the free pages if the transaction allocated it
//! itself, and into this commit's retained group if it was committed: a reader
//! or a recovery may still need it. `design/commits-and-recovery.md` is the
//! specification.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::sync::Arc;

use crate::btree::Store;
use crate::error::Result;
use crate::format::Check;
use crate::storage::Pager;

/// The file never reaches this many bytes: the lock bytes start there.
const LOCK_BASE: u64 = 1 << 62;

/// The free space of one write transaction.
#[derive(Debug)]
pub(crate) struct Space {
    pager: Arc<Pager>,
    txn: u64,
    /// Runs of free pages: first page and length. Runs never overlap or touch.
    free: BTreeMap<u64, u64>,
    page_count: u64,
    max_page_count: u64,
    /// Pages this transaction allocated.
    fresh: HashSet<u64>,
    /// Committed pages this transaction stopped using: its retained group.
    retired: Vec<u64>,
    /// Counts every allocation and release, so the commit can tell when its
    /// changes to the allocator trees have stopped moving pages.
    changes: u64,
    /// Set while the commit settles the allocator trees; see [`Space::settle`].
    settling: bool,
}

impl Space {
    /// The space of a transaction `txn` whose base commit has `page_count`
    /// pages and the given free runs.
    pub(crate) fn new(
        pager: Arc<Pager>,
        txn: u64,
        page_count: u64,
        free: BTreeMap<u64, u64>,
    ) -> Self {
        let max_page_count = LOCK_BASE / pager.page_size() as u64;

        Self {
            pager,
            txn,
            free,
            page_count,
            max_page_count,
            fresh: HashSet::new(),
            retired: Vec::new(),
            changes: 0,
            settling: false,
        }
    }

    /// Switches to how the commit settles the allocator trees: from here on,
    /// a page this transaction allocated and then released joins the retained
    /// group instead of becoming free again.
    ///
    /// That keeps the free pages from growing while the free tree is being
    /// written to match them. A free tree that both needs pages and records
    /// them would otherwise chase itself: emptied, it gives its page back;
    /// given that page back, it needs a page to record it in. The pages retained
    /// this way are reclaimed by a later commit like any other.
    pub(crate) fn settle(&mut self) {
        self.settling = true;
    }

    /// Makes a run of pages free, merging it with the runs it touches.
    pub(crate) fn add_free(&mut self, start: u64, len: u64) {
        let mut start = start;
        let mut len = len;

        if let Some((&before, &before_len)) = self.free.range(..start).next_back() {
            if before + before_len == start {
                self.free.remove(&before);
                start = before;
                len += before_len;
            }
        }

        if let Some(&after_len) = self.free.get(&(start + len)) {
            self.free.remove(&(start + len));
            len += after_len;
        }

        self.free.insert(start, len);
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

            self.free.remove(&start);
            self.page_count = start;
            self.changes += 1;
        }
    }

    pub(crate) fn free(&self) -> &BTreeMap<u64, u64> {
        &self.free
    }

    pub(crate) fn page_count(&self) -> u64 {
        self.page_count
    }

    pub(crate) fn changes(&self) -> u64 {
        self.changes
    }

    /// This commit's retained group, as runs of consecutive pages.
    pub(crate) fn retired_runs(&self) -> Vec<(u64, u32)> {
        let mut pages = self.retired.clone();
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

    fn allocate(&mut self) -> Result<u64> {
        let page = match self.free.pop_first() {
            Some((start, len)) => {
                if len > 1 {
                    self.free.insert(start + 1, len - 1);
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
                self.free.remove(&start);

                if len > pages {
                    self.free.insert(start + pages, len - pages);
                }

                start
            }
            None => self.extend(pages)?,
        };

        self.fresh.extend(first..first + pages);
        self.changes += 1;

        Ok(first)
    }

    fn release(&mut self, page: u64) {
        if self.fresh.remove(&page) && !self.settling {
            self.add_free(page, 1);
        } else {
            debug_assert!(!self.retired.contains(&page), "page {page} released twice");

            // A page of this transaction released while settling, or a
            // committed page: either way, retained.
            self.retired.push(page);
            self.changes += 1;
        }
    }

    fn write_page(&mut self, page: u64, bytes: &mut [u8]) -> Result<Check> {
        self.pager.write(page, bytes)
    }
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
        ));

        Space::new(pager, 2, page_count, free.iter().copied().collect())
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

        space.release(fresh);
        space.release(4);

        assert_eq!(space.free(), &[(fresh, 1)].into_iter().collect());
        assert_eq!(space.retired_runs(), [(4, 1)]);
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
    fn retired_pages_are_grouped_into_runs() {
        let mut space = space(20, &[]);

        for page in [9, 3, 4, 5, 12, 10] {
            space.release(page);
        }

        assert_eq!(space.retired_runs(), [(3, 3), (9, 2), (12, 1)]);
    }
}
