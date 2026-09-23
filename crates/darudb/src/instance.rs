//! The one instance of each open database file in this process.
//!
//! Every [`Database`](crate::Database) handle for a file in the process shares
//! it: the file handle, the page cache, the published commit, the registry of
//! snapshots in use, and the writer gate. A second handle to the same file is
//! a second reference to this instance, never a second open file, which is
//! what `design/locking.md` requires for the file locks of phase 3 and what
//! keeps two handles in one process from writing at once today.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::btree::{LoadedNode, Loader};
use crate::error::{Error, Result};
use crate::format::{CommitRecord, SLOT_COUNT, Selector, StaticHeader};
use crate::storage::{Cache, Pager};

/// How many decoded pages each open file keeps in memory.
const CACHE_PAGES: usize = 4096;

/// The committed state every transaction starts from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    pub(crate) selector: Selector,
    pub(crate) records: [Option<CommitRecord>; SLOT_COUNT],
}

impl Header {
    /// The commit the selector names.
    pub(crate) fn published(&self) -> Result<CommitRecord> {
        self.records[self.selector.slot].ok_or_else(|| Error::Internal {
            message: "the published slot is empty".to_owned(),
        })
    }
}

/// One open database file, shared by every handle to it in this process.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) path: PathBuf,
    pub(crate) static_header: StaticHeader,
    pub(crate) pager: Arc<Pager>,
    pub(crate) loader: Loader,
    pub(crate) cache: Arc<Cache<LoadedNode>>,
    pub(crate) busy_timeout: Duration,
    header: Mutex<Header>,
    writer: Mutex<bool>,
    writer_free: Condvar,
    snapshots: Mutex<BTreeMap<u64, usize>>,
    sync_failed: AtomicBool,
}

impl Shared {
    /// An instance whose header is not known yet: recovery reads it, and
    /// [`set_header`](Self::set_header) stores it.
    pub(crate) fn new(
        pager: Arc<Pager>,
        path: PathBuf,
        static_header: StaticHeader,
        busy_timeout: Duration,
    ) -> Self {
        let header = Header {
            selector: Selector {
                slot: 0,
                unsynced: false,
            },
            records: [None; SLOT_COUNT],
        };
        let cache = Arc::new(Cache::new(CACHE_PAGES));
        let loader = Loader::new(Arc::clone(&pager), Arc::clone(&cache));

        Self {
            path,
            static_header,
            pager,
            loader,
            cache,
            busy_timeout,
            header: Mutex::new(header),
            writer: Mutex::new(false),
            writer_free: Condvar::new(),
            snapshots: Mutex::new(BTreeMap::new()),
            sync_failed: AtomicBool::new(false),
        }
    }

    /// The committed state as the last commit left it.
    pub(crate) fn header(&self) -> Header {
        *lock(&self.header)
    }

    pub(crate) fn set_header(&self, header: Header) {
        *lock(&self.header) = header;
    }

    /// Takes a snapshot of the published commit and registers it, so that no
    /// writer reuses a page it can reach until it is released.
    pub(crate) fn begin_snapshot(&self) -> Result<CommitRecord> {
        self.check_usable()?;

        // The header lock is held while registering, so a writer cannot
        // publish and reclaim between the read and the registration.
        let header = lock(&self.header);
        let record = header.published()?;

        *lock(&self.snapshots).entry(record.txn).or_insert(0) += 1;

        Ok(record)
    }

    pub(crate) fn end_snapshot(&self, txn: u64) {
        let mut snapshots = lock(&self.snapshots);

        if let Some(count) = snapshots.get_mut(&txn) {
            *count -= 1;

            if *count == 0 {
                snapshots.remove(&txn);
            }
        }
    }

    /// The oldest snapshot in use in this process, if any.
    pub(crate) fn oldest_snapshot(&self) -> Option<u64> {
        lock(&self.snapshots).keys().next().copied()
    }

    /// Waits for this process's writer gate, up to the busy timeout.
    pub(crate) fn acquire_writer(self: &Arc<Self>) -> Result<WriterGuard> {
        self.check_usable()?;

        let deadline = Instant::now() + self.busy_timeout;
        let mut busy = lock(&self.writer);

        while *busy {
            let now = Instant::now();

            if now >= deadline {
                return Err(Error::Busy {
                    path: self.path.clone(),
                });
            }

            busy = self
                .writer_free
                .wait_timeout(busy, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }

        *busy = true;

        Ok(WriterGuard {
            shared: Arc::clone(self),
        })
    }

    /// Marks the file unusable after a failed barrier.
    pub(crate) fn fail_sync(&self) {
        self.sync_failed.store(true, Ordering::SeqCst);
    }

    /// Refuses every use of a file whose barrier failed.
    pub(crate) fn check_usable(&self) -> Result<()> {
        if self.sync_failed.load(Ordering::SeqCst) {
            return Err(Error::SyncFailed {
                path: self.path.clone(),
                source: None,
            });
        }

        Ok(())
    }
}

/// Holds this process's writer gate until dropped.
#[derive(Debug)]
pub(crate) struct WriterGuard {
    shared: Arc<Shared>,
}

impl Drop for WriterGuard {
    fn drop(&mut self) {
        *lock(&self.shared.writer) = false;
        self.shared.writer_free.notify_one();
    }
}

/// Locks a mutex, carrying on if a thread panicked while holding it: every
/// value behind these locks is left consistent between statements.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What identifies a file: the same file reached through another path, a
/// link or a different spelling, has the same key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FileKey(Identity);

#[cfg(unix)]
type Identity = (u64, u64);

#[cfg(windows)]
type Identity = PathBuf;

impl FileKey {
    /// The key of the file at `path`, if something is there.
    pub(crate) fn of(path: &Path) -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let metadata = fs::metadata(path).ok()?;

            Some(Self((metadata.dev(), metadata.ino())))
        }

        // The volume serial number and file index that would identify a file
        // on Windows are not available from the standard library yet, so the
        // canonical path stands in for them. Two hard links to one database
        // are therefore two instances on Windows.
        #[cfg(windows)]
        {
            fs::canonicalize(path).ok().map(Self)
        }
    }
}

/// The instances open in this process.
pub(crate) static REGISTRY: LazyLock<Mutex<HashMap<FileKey, Weak<Shared>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Locks the registry. Opening holds it from the lookup to the insertion, so
/// two threads opening one file end up with one instance.
pub(crate) fn registry() -> MutexGuard<'static, HashMap<FileKey, Weak<Shared>>> {
    lock(&REGISTRY)
}
