//! The one instance of each open database file in this process.
//!
//! Every [`Database`](crate::Database) handle for a file in the process shares
//! it: the file handle and the locks on it, the page cache, the published
//! commit, the registry of snapshots in use, and the writer gate. A second
//! handle to the same file is a second reference to this instance, never a
//! second open file: closing a second descriptor of the file would release
//! every lock the process holds on it (`design/locking.md`).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use crate::btree::{LoadedNode, Loader};
use crate::crypto::{DataKey, PasswordCost, RecordAuth, Secret, Unlocker};
use crate::error::{Error, Result};
use crate::format::{
    CommitRecord, HEADER_LEN, KeyBlock, SELECTOR_OFFSET, SLOT_COUNT, Selector, StaticHeader,
    slot_offset,
};
use crate::lock::{LockError, Locks};
use crate::storage::{Cache, DbFile, Pager};

/// How many decoded pages each open file keeps in memory.
const CACHE_PAGES: usize = 4096;

/// How long the thread that ends a due window waits for a running writer
/// before it looks at the window again.
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// How many times a reader reads the header before it takes a published
/// record that fails its check for damage rather than for a stale selector.
const HEADER_ATTEMPTS: usize = 8;

/// The options every handle to one file shares: those of the handle that
/// opened it first.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Settings {
    pub(crate) busy_timeout: Duration,
    pub(crate) max_unsynced_pages: u64,
    pub(crate) max_unsynced_time: Duration,
    pub(crate) password_cost: PasswordCost,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// This process's handles to the file and its locks on it.
    locks: Locks,
    pub(crate) loader: Loader,
    pub(crate) cache: Arc<Cache<LoadedNode>>,
    pub(crate) settings: Settings,
    /// The data key of an encrypted file, for wrapping it under a new key.
    pub(crate) data_key: Option<DataKey>,
    /// The key that signs an encrypted file's commit records.
    pub(crate) record_auth: Option<RecordAuth>,
    /// The header as this instance last wrote it or read it while holding the
    /// writer lock. Only a writer changes it.
    header: Mutex<Header>,
    /// The last selector written before the last barrier, which a power cut
    /// can bring back. `None` until this instance's first barrier, and again
    /// whenever another process has written the header since: it may have
    /// written a selector with no barrier after it.
    last_barrier: Mutex<Option<Selector>>,
    unsynced: Mutex<Unsynced>,
    /// The free runs of the commit with the given transaction id, which the
    /// last write transaction left for the next, so that it need not read the
    /// free tree again.
    free_runs: Mutex<Option<(u64, BTreeMap<u64, u64>)>>,
    writer: Mutex<bool>,
    writer_free: Condvar,
    sync_failed: AtomicBool,
    /// Declared last, so that it is dropped last, once the handle has closed
    /// and the locks are released; see [`Hold`].
    hold: Arc<Hold>,
}

/// Lives for exactly as long as an instance holds its file. The registry keeps
/// a weak reference to it, which tells an instance that is still closing from
/// one that is gone.
#[derive(Debug)]
pub(crate) struct Hold;

impl Shared {
    /// An instance whose header is not known yet: recovery reads it, and
    /// [`set_header`](Self::set_header) stores it.
    pub(crate) fn new(
        pager: Arc<Pager>,
        locks: Locks,
        path: PathBuf,
        static_header: StaticHeader,
        settings: Settings,
        data_key: Option<DataKey>,
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
            locks,
            loader,
            cache,
            settings,
            record_auth: data_key.as_ref().map(RecordAuth::new),
            data_key,
            header: Mutex::new(header),
            last_barrier: Mutex::new(None),
            unsynced: Mutex::new(Unsynced::default()),
            free_runs: Mutex::new(None),
            writer: Mutex::new(false),
            writer_free: Condvar::new(),
            sync_failed: AtomicBool::new(false),
            hold: Arc::new(Hold),
        }
    }

    /// Whether `secret` is what opening this file takes: nothing for a plain
    /// file, and a key or password that unwraps a key block of the file's
    /// records for an encrypted one. Another handle to an open file is let in
    /// on the same terms as the first.
    pub(crate) fn admit(&self, secret: Option<&Secret>) -> Result<()> {
        let path = self.path.clone();

        match (secret, self.data_key.is_some()) {
            (None, false) => Ok(()),
            (Some(_), false) => Err(Error::InvalidArgument {
                message: format!(
                    "`{}` is not encrypted, so it cannot be opened with a key",
                    path.display()
                ),
            }),
            (None, true) => Err(Error::KeyRequired { path }),
            (Some(secret), true) => {
                let mut unlocker = Unlocker::new(secret);
                // Another process may have changed the key since this one
                // opened the file, so the records come from the file.
                let bytes = self.pager.read_header(HEADER_LEN)?;

                for record in self.records_in(&bytes).iter().flatten() {
                    let Ok(Some(block)) = KeyBlock::decode(&record.key_block) else {
                        continue;
                    };

                    if let Ok(Some(_)) = unlocker.unlock(&block, &self.static_header.file_id) {
                        return Ok(());
                    }
                }

                Err(Error::WrongKey { path })
            }
        }
    }

    /// Keeps `file`, a second handle to this file, open until the instance
    /// closes; see [`Locks::keep`].
    pub(crate) fn keep_handle(&self, file: Arc<DbFile>) {
        self.locks.keep(file);
    }

    /// Whether this is a process forked from the one that opened the file. It
    /// holds none of the file's locks, so its inherited handles behave as
    /// closed, and it opens the file again to use it.
    pub(crate) fn inherited(&self) -> bool {
        self.locks.inherited()
    }

    /// Sets the record MAC of `record` for slot `slot`, in an encrypted file.
    pub(crate) fn sign_record(&self, slot: usize, record: &mut CommitRecord) {
        if let Some(auth) = &self.record_auth {
            record.mac = auth.mac(&self.static_header.file_id, slot, &record.authenticated());
        }
    }

    /// The committed state as this instance's last writer left it.
    pub(crate) fn header(&self) -> Header {
        *lock(&self.header)
    }

    /// Reads the header from the file, and keeps it as the state the next
    /// commit starts from. The caller holds the writer lock, so no other
    /// process can change it meanwhile.
    ///
    /// If it is not the header this instance last knew, another process has
    /// written it since, and the selector a power cut would bring back is no
    /// longer known (`design/commits-and-recovery.md`, "Choosing the slot").
    pub(crate) fn refresh_header(&self) -> Result<Header> {
        let bytes = self.pager.read_header(HEADER_LEN)?;
        let selector = Selector::decode(bytes[SELECTOR_OFFSET])
            .map_err(|reason| self.pager.corrupted(reason.to_owned()))?;
        let header = Header {
            selector,
            records: self.records_in(&bytes),
        };

        // Only a writer changes the header, and this process holds the writer
        // lock: a published record that fails is damaged, not being written.
        if header.records[selector.slot].is_none() {
            return Err(self
                .pager
                .corrupted("the published commit record fails its check".to_owned()));
        }

        let mut known = lock(&self.header);

        if *known != header {
            *lock(&self.last_barrier) = None;
            *known = header;
        }

        Ok(header)
    }

    /// The valid records of the header `bytes`, signed included. A slot whose
    /// record fails is as good as empty: a writer that died while filling it
    /// leaves one like that.
    fn records_in(&self, bytes: &[u8]) -> [Option<CommitRecord>; SLOT_COUNT] {
        let mut records = [None; SLOT_COUNT];

        for (slot, record) in records.iter_mut().enumerate() {
            *record = CommitRecord::decode(slot, &bytes[slot_offset(slot)..])
                .ok()
                .flatten()
                .filter(|record| self.signed(slot, record));
        }

        records
    }

    pub(crate) fn set_header(&self, header: Header) {
        *lock(&self.header) = header;
    }

    /// Takes a snapshot of the published commit and registers it, so that no
    /// writer, in this process or another, reuses a page it can reach until
    /// it is released.
    ///
    /// No lock guards the header. The reader reads it from the file, registers
    /// the snapshot, and reads it again: a writer that reclaims pages the
    /// snapshot can reach starts after a commit newer than the snapshot is
    /// published, and so after the second read, which saw the snapshot still
    /// published, and after the registration, which the writer therefore sees
    /// (`design/locking.md`, "Beginning a read").
    pub(crate) fn begin_snapshot(&self) -> Result<CommitRecord> {
        self.check_owner()?;
        self.check_usable()?;

        let deadline = Instant::now().checked_add(self.settings.busy_timeout);

        loop {
            let (bytes, slot, record) = self.read_published()?;

            // The multi-process suite pauses here in some workers, to test
            // the second read below.
            #[cfg(test)]
            crate::testing::widen_race();

            self.locks
                .register(record.txn, deadline)
                .map_err(|error| self.lock_error(error))?;

            // A snapshot left registered would hold pages back for as long as
            // the file is open.
            let again = match self.pager.read_header(slot_offset(slot) + 8) {
                Ok(again) => again,
                Err(error) => {
                    self.locks.unregister(record.txn);

                    return Err(error);
                }
            };
            let txn = again[slot_offset(slot)..]
                .first_chunk::<8>()
                .map(|bytes| u64::from_le_bytes(*bytes));

            if again[SELECTOR_OFFSET] == bytes[SELECTOR_OFFSET] && txn == Some(record.txn) {
                return Ok(record);
            }

            self.locks.unregister(record.txn);
        }
    }

    /// Reads the header from the file, without a lock, until its published
    /// record is valid. Returns the header's bytes, the published slot and its
    /// record.
    ///
    /// A reader may hold a stale selector that names the slot a writer is
    /// filling, so a record that fails is read again, and only one that keeps
    /// failing is damaged.
    pub(crate) fn read_published(&self) -> Result<(Vec<u8>, usize, CommitRecord)> {
        let mut damaged = 0;

        loop {
            let bytes = self.pager.read_header(HEADER_LEN)?;

            match self.published_in(&bytes) {
                Ok((slot, record)) => return Ok((bytes, slot, record)),
                Err(reason) => {
                    damaged += 1;

                    if damaged == HEADER_ATTEMPTS {
                        return Err(self.pager.corrupted(reason.to_owned()));
                    }

                    thread::yield_now();
                }
            }
        }
    }

    /// Lets other processes open the file, once recovery is done.
    pub(crate) fn share_open_lock(&self) -> Result<()> {
        self.locks
            .share(self.settings.busy_timeout)
            .map_err(|error| self.lock_error(error))
    }

    /// The slot of the published commit in the header `bytes`, and its record
    /// once it is known to be valid, signed included.
    fn published_in(
        &self,
        bytes: &[u8],
    ) -> std::result::Result<(usize, CommitRecord), &'static str> {
        let selector = Selector::decode(bytes[SELECTOR_OFFSET])?;
        let record = CommitRecord::decode(selector.slot, &bytes[slot_offset(selector.slot)..])?
            .ok_or("the selector names an empty slot")?;

        if !self.signed(selector.slot, &record) {
            return Err("the published commit record fails its MAC");
        }

        Ok((selector.slot, record))
    }

    /// Whether `record`, read from slot `slot`, carries the MAC of this file's
    /// data key, as every record of an encrypted file has to. A plain file's
    /// records have none.
    pub(crate) fn signed(&self, slot: usize, record: &CommitRecord) -> bool {
        self.record_auth.as_ref().is_none_or(|auth| {
            auth.verify(
                &self.static_header.file_id,
                slot,
                &record.authenticated(),
                &record.mac,
            )
        })
    }

    pub(crate) fn end_snapshot(&self, txn: u64) {
        self.locks.unregister(txn);
    }

    /// The largest of `groups`, retained group ids in ascending order, that no
    /// registered snapshot in any process can reach; see
    /// [`Locks::reclaimable`]. The caller holds the writer lock.
    pub(crate) fn reclaimable(&self, groups: &[u64]) -> Result<Option<u64>> {
        self.locks
            .reclaimable(groups)
            .map_err(|error| self.lock_error(error))
    }

    /// A lock that was not taken, as the error the caller sees. A file system
    /// that reports it has no working locks is one the engine cannot use.
    pub(crate) fn lock_error(&self, error: LockError) -> Error {
        match error {
            LockError::Busy => Error::Busy {
                path: self.path.clone(),
            },
            LockError::Io(source) if source.kind() == std::io::ErrorKind::Unsupported => {
                Error::UnsupportedFileSystem {
                    path: self.path.clone(),
                }
            }
            LockError::Io(source) => self.pager.io_error(source),
        }
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

    /// Waits for this process's writer gate and then for the writer lock,
    /// together up to the busy timeout.
    pub(crate) fn acquire_writer(self: &Arc<Self>) -> Result<WriterGuard> {
        self.acquire_writer_within(self.settings.busy_timeout)
    }

    /// Waits for this process's writer gate and then for the writer lock,
    /// together up to `timeout`. The gate lets one thread at a time compete
    /// for the lock with other processes.
    fn acquire_writer_within(self: &Arc<Self>, timeout: Duration) -> Result<WriterGuard> {
        self.check_owner()?;
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
        drop(busy);

        if let Err(error) = self.locks.lock_writer(deadline) {
            self.release_gate();

            return Err(self.lock_error(error));
        }

        Ok(WriterGuard {
            shared: Arc::clone(self),
        })
    }

    /// Opens this process's writer gate to the next thread.
    fn release_gate(&self) {
        *lock(&self.writer) = false;
        self.writer_free.notify_one();
    }

    /// Makes every commit so far durable, deferred ones included, by any
    /// process: a barrier and the selector with the unsynced bit clear, if the
    /// file's selector has it set.
    ///
    /// A selector with the bit clear was written after a barrier that made
    /// the commit it names durable, and every commit before it. Finding one
    /// needs no writer lock, which keeps `close` from waiting for another
    /// process's write transaction when there is nothing to do.
    pub(crate) fn sync(self: &Arc<Self>) -> Result<()> {
        self.check_owner()?;
        self.check_usable()?;

        let bytes = self.pager.read_header(SELECTOR_OFFSET + 1)?;

        if Selector::decode(bytes[SELECTOR_OFFSET]).is_ok_and(|selector| !selector.unsynced) {
            self.close_window();

            return Ok(());
        }

        let _writer = self.acquire_writer()?;

        self.sync_published()
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
    /// selector with the unsynced bit clear. The caller holds the writer lock,
    /// and the header comes from the file: another process may have committed
    /// since this one last wrote it.
    pub(crate) fn sync_published(&self) -> Result<()> {
        self.check_usable()?;

        let mut header = self.refresh_header()?;

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

    /// Refuses a handle inherited by a forked process, which holds none of the
    /// file's locks. Checked where a transaction begins or commits, which is
    /// where the locks would be needed.
    pub(crate) fn check_owner(&self) -> Result<()> {
        if self.inherited() {
            return Err(Error::Closed);
        }

        Ok(())
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
        // A forked process leaves the file to the process that opened it.
        if self.inherited() {
            return;
        }

        // Only this process's own deferred commits are its business here. No
        // other handle is left, so the writer gate is free; the writer lock
        // may take waiting for.
        if lock(&self.unsynced).window.is_some() {
            let deadline = Instant::now().checked_add(self.settings.busy_timeout);

            if self.locks.lock_writer(deadline).is_ok() {
                let _ = self.sync_published();

                self.locks.unlock_writer();
            }
        }

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
        self.shared.locks.unlock_writer();
        self.shared.release_gate();
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
    /// The key of the file at `path`, if something is there. Nothing is
    /// opened to find it.
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
        // are therefore two instances on Windows, which is safe there: a lock
        // belongs to a handle, so the two instances take turns through their
        // locks as two processes would.
        #[cfg(windows)]
        {
            fs::canonicalize(path).ok().map(Self)
        }
    }

    /// The key of the open file `file`, which `path` led to.
    pub(crate) fn of_file(file: &DbFile, path: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let _ = path;
            let metadata = file.as_file().metadata()?;

            Ok(Self((metadata.dev(), metadata.ino())))
        }

        #[cfg(windows)]
        {
            let _ = file;

            fs::canonicalize(path).map(Self)
        }
    }
}

/// The registry's entry for one open file.
#[derive(Debug)]
pub(crate) struct Entry {
    instance: Weak<Shared>,
    hold: Weak<Hold>,
}

impl Entry {
    pub(crate) fn of(shared: &Arc<Shared>) -> Self {
        Self {
            instance: Arc::downgrade(shared),
            hold: Arc::downgrade(&shared.hold),
        }
    }

    /// Whether the instance still holds its file, closing or not.
    pub(crate) fn holds(&self) -> bool {
        self.hold.strong_count() > 0
    }
}

/// How long an opening thread waits between two looks at an instance that is
/// closing.
const CLOSING_PAUSE: Duration = Duration::from_millis(1);

/// The instances open in this process.
pub(crate) static REGISTRY: LazyLock<Mutex<HashMap<FileKey, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Locks the registry. Opening holds it from the lookup to the insertion, so
/// two threads opening one file end up with one instance.
pub(crate) fn registry() -> MutexGuard<'static, HashMap<FileKey, Entry>> {
    lock(&REGISTRY)
}

/// The instance of the file `key` names, if this process has it open. One a
/// forked process inherited does not count: it holds none of the locks.
///
/// An instance whose last handle is gone may still be closing: ending its
/// unsynced window, which can wait for another process's writer, and then
/// releasing its locks and closing its handle. Opening the file again
/// meanwhile would take the locks through a second handle, and on a Unix-like
/// system the first one's unlock and close would release them all. So this
/// waits for the closing to finish, and then there is no instance. The
/// closing never needs the registry, so it cannot wait for the caller.
pub(crate) fn find(instances: &HashMap<FileKey, Entry>, key: &FileKey) -> Option<Arc<Shared>> {
    let entry = instances.get(key)?;

    if let Some(shared) = entry.instance.upgrade() {
        return (!shared.inherited()).then_some(shared);
    }

    while entry.holds() {
        thread::sleep(CLOSING_PAUSE);
    }

    None
}
