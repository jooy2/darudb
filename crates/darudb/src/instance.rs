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
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use crate::btree::{LoadedNode, Loader};
use crate::error::{Error, Result};
use crate::format::{CommitRecord, SLOT_COUNT, Selector, StaticHeader};
use crate::storage::{Cache, Pager};

/// How many decoded pages each open file keeps in memory.
const CACHE_PAGES: usize = 4096;

/// How long the thread that ends a due window waits for a running writer
/// before it looks at the window again.
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// The options every handle to one file shares: those of the handle that
/// opened it first.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Settings {
    pub(crate) busy_timeout: Duration,
    pub(crate) max_unsynced_pages: u64,
    pub(crate) max_unsynced_time: Duration,
}

/// The deferred commits since the last barrier.
#[derive(Debug, Clone, Copy)]
struct Window {
    opened: Instant,
    pages: u64,
}

/// The unsynced window, and the thread that ends it when it is due.
#[derive(Debug, Default)]
struct Unsynced {
    window: Option<Window>,
    flusher: Option<Thread>,
}

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
    pub(crate) settings: Settings,
    header: Mutex<Header>,
    /// The last selector written before the last barrier, which a power cut
    /// can bring back. `None` until this instance's first barrier: another
    /// process may have written the selector the file shows without one.
    last_barrier: Mutex<Option<Selector>>,
    unsynced: Mutex<Unsynced>,
    /// The free runs of the commit with the given transaction id, which the
    /// last write transaction left for the next, so that it need not read the
    /// free tree again.
    free_runs: Mutex<Option<(u64, BTreeMap<u64, u64>)>>,
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
        settings: Settings,
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
            settings,
            header: Mutex::new(header),
            last_barrier: Mutex::new(None),
            unsynced: Mutex::new(Unsynced::default()),
            free_runs: Mutex::new(None),
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

    /// Issues a barrier, and records the selector it made durable. A failed
    /// barrier makes the file unusable.
    pub(crate) fn barrier(&self) -> Result<()> {
        let selector = self.header().selector;

        if let Err(source) = self.pager.sync() {
            self.fail_sync();

            return Err(Error::SyncFailed {
                path: self.path.clone(),
                source: Some(source),
            });
        }

        *lock(&self.last_barrier) = Some(selector);

        Ok(())
    }

    /// The selector a power cut can bring back, if this instance knows it.
    pub(crate) fn last_barrier(&self) -> Option<Selector> {
        *lock(&self.last_barrier)
    }

    /// Records the selector recovery made durable when it opened the file.
    pub(crate) fn set_last_barrier(&self, selector: Option<Selector>) {
        *lock(&self.last_barrier) = selector;
    }

    /// The free runs of commit `txn`, if the last write transaction left
    /// them. Runs left for any other commit are thrown away.
    pub(crate) fn take_free_runs(&self, txn: u64) -> Option<BTreeMap<u64, u64>> {
        lock(&self.free_runs)
            .take()
            .and_then(|(of, runs)| (of == txn).then_some(runs))
    }

    /// Leaves the free runs of commit `txn` for the next write transaction.
    pub(crate) fn leave_free_runs(&self, txn: u64, runs: BTreeMap<u64, u64>) {
        *lock(&self.free_runs) = Some((txn, runs));
    }

    /// Waits for this process's writer gate, up to the busy timeout.
    pub(crate) fn acquire_writer(self: &Arc<Self>) -> Result<WriterGuard> {
        self.acquire_writer_within(self.settings.busy_timeout)
    }

    /// Waits for this process's writer gate, up to `timeout`.
    fn acquire_writer_within(self: &Arc<Self>, timeout: Duration) -> Result<WriterGuard> {
        self.check_usable()?;

        // A timeout too long to add up is as good as waiting for ever.
        let deadline = Instant::now().checked_add(timeout);
        let mut busy = lock(&self.writer);

        while *busy {
            let Some(deadline) = deadline else {
                busy = self
                    .writer_free
                    .wait(busy)
                    .unwrap_or_else(PoisonError::into_inner);

                continue;
            };
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

    /// Whether a deferred commit writing `pages` pages may stay deferred, or
    /// has to be made durable because the window would pass its limits.
    pub(crate) fn may_defer(&self, pages: u64) -> bool {
        let limits = &self.settings;

        match lock(&self.unsynced).window {
            None => pages <= limits.max_unsynced_pages,
            Some(window) => {
                window.pages + pages <= limits.max_unsynced_pages
                    && window.opened.elapsed() < limits.max_unsynced_time
            }
        }
    }

    /// Records a deferred commit of `pages` pages in the window, and makes
    /// sure a thread will end the window when it is due.
    pub(crate) fn extend_window(self: &Arc<Self>, pages: u64) {
        let mut unsynced = lock(&self.unsynced);
        let window = unsynced.window.get_or_insert(Window {
            opened: Instant::now(),
            pages: 0,
        });

        window.pages += pages;

        if unsynced.flusher.is_none() && self.window_due(&unsynced).is_some() {
            let shared = Arc::downgrade(self);
            let spawned = thread::Builder::new()
                .name("darudb-sync".to_owned())
                .spawn(move || flush_when_due(&shared));

            // Without the thread, the next deferred commit after the time is
            // up still ends the window; nothing else is lost.
            if let Ok(handle) = spawned {
                unsynced.flusher = Some(handle.thread().clone());
            }
        }
    }

    /// Records that a barrier made every commit so far durable.
    pub(crate) fn close_window(&self) {
        let mut unsynced = lock(&self.unsynced);

        unsynced.window = None;

        if let Some(flusher) = &unsynced.flusher {
            flusher.unpark();
        }
    }

    /// When the open window, if any, has to end. `None` also for a time limit
    /// too long to reach.
    fn window_due(&self, unsynced: &Unsynced) -> Option<Instant> {
        unsynced
            .window
            .and_then(|window| window.opened.checked_add(self.settings.max_unsynced_time))
    }

    /// Makes the published commit durable if it is not: a barrier, then the
    /// selector with the unsynced bit clear. The caller holds the writer gate.
    pub(crate) fn sync_published(&self) -> Result<()> {
        self.check_usable()?;

        let mut header = self.header();

        if !header.selector.unsynced {
            self.close_window();

            return Ok(());
        }

        self.barrier()?;

        header.selector.unsynced = false;

        if let Err(error) = self
            .pager
            .write_header(&[header.selector.encode()], crate::format::SELECTOR_OFFSET)
        {
            // The commits are durable already; only the flag saying so did not
            // reach the file. Recovery finds out by checking them.
            self.fail_sync();

            return Err(error);
        }

        self.set_header(header);
        self.close_window();

        Ok(())
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

impl Drop for Shared {
    /// The last handle to the file is gone. Deferred commits still waiting
    /// for a barrier get one, on a best-effort basis: nothing is left to report
    /// a failure to, and a failure costs nothing but what a power cut would.
    fn drop(&mut self) {
        let _ = self.sync_published();

        if let Some(flusher) = lock(&self.unsynced).flusher.take() {
            flusher.unpark();
        }
    }
}

/// The body of the thread that ends the unsynced window when it is due.
///
/// It holds the instance only while it works, so it never keeps a file open
/// that every handle has let go of. It stops when there is no window left to
/// end, the instance is gone, or the file became unusable.
fn flush_when_due(shared: &Weak<Shared>) {
    loop {
        let Some(instance) = shared.upgrade() else {
            return;
        };
        let due = {
            let mut unsynced = lock(&instance.unsynced);

            match instance.window_due(&unsynced) {
                Some(due) => due,
                None => {
                    unsynced.flusher = None;

                    return;
                }
            }
        };
        let now = Instant::now();

        if now < due {
            drop(instance);
            thread::park_timeout(due - now);

            continue;
        }

        let ended = match instance.acquire_writer_within(FLUSH_WAIT) {
            Ok(_writer) => instance.sync_published(),
            // A writer is still running. Its own commit may end the window;
            // otherwise the next round tries again.
            Err(Error::Busy { .. }) => Ok(()),
            Err(error) => Err(error),
        };

        if ended.is_err() {
            lock(&instance.unsynced).flusher = None;

            return;
        }
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
