//! Coordination between the processes that share one database file, through
//! the operating system's byte-range locks and nothing else.
//!
//! `design/locking.md` is the specification. The locks sit on bytes far past
//! the end of any data:
//!
//! | Byte            | Lock                                                         |
//! | --------------- | ------------------------------------------------------------ |
//! | 2^62            | Open: shared while the file is open, exclusive to recover it |
//! | 2^62 + 1        | Writer: held by the process whose transaction is writing     |
//! | 2^62 + 64 + `s` | Snapshot `s`: shared by every process reading that snapshot  |
//!
//! No mutex lives in shared memory and no lock file has a layout, so a process
//! that dies leaves nothing for the others to clean up: the operating system
//! releases its locks, and that is all.
//!
//! On Windows, byte-range locks are mandatory: bytes locked through one handle
//! cannot be read or written through another. The file never reaches byte
//! 2^62, so no read or write of it ever touches a lock byte, and the locks
//! never stand in the way of the data.
//!
//! A record lock on a Unix-like system belongs to the process, and closing
//! **any** descriptor of the file releases all of the process's locks on it.
//! That is why the instance in `instance.rs` holds the only handle to the file
//! in the process, and why [`Locks`] keeps every handle it is given open until
//! it is dropped.
//!
//! The open lock is how a process that opens the file learns whether any other
//! process has it open, which decides whether a crash may have left something
//! to recover. The first to open it holds it exclusively while it recovers,
//! and every process shares it after that.

mod sys;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sys::Mode;

use crate::format::TXN_LIMIT;
use crate::storage::DbFile;

/// The open lock's byte, 2^62, the first of the lock bytes.
const OPEN_BYTE: u64 = 1 << 62;

/// The writer lock's byte.
const WRITER_BYTE: u64 = OPEN_BYTE + 1;

/// The lock byte of snapshot 0. Snapshot `s` is locked at this byte plus `s`,
/// which stays below 2^63 because every transaction id is below
/// [`TXN_LIMIT`].
const SNAPSHOT_BASE: u64 = OPEN_BYTE + 64;

/// The first pause between two attempts at a lock another process holds.
const FIRST_PAUSE: Duration = Duration::from_micros(20);

/// The longest pause between two attempts: short enough to notice a released
/// lock soon, long enough not to keep a processor busy.
const LAST_PAUSE: Duration = Duration::from_millis(10);

/// Whether the database at `path` is on a network file system, where the
/// engine refuses to open it: such file systems break the locks and the
/// barriers the engine relies on (`design/README.md`). `file` is the database
/// file once it is open; before, the directory it is created in is checked,
/// so that nothing is created there. A best effort; see [`sys::is_remote`].
pub(crate) fn on_network_file_system(path: &Path, file: Option<&DbFile>) -> bool {
    sys::is_remote(file.map(DbFile::as_file), path)
}

/// How the open lock was granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Exclusively: no other process has the file open, and recovery may run.
    /// [`Locks::share`] lets other processes in afterwards.
    Alone,
    /// Shared with other processes that have the file open. Whatever they
    /// found to recover, the first of them recovered.
    Shared,
}

/// Why a lock was not taken.
#[derive(Debug)]
pub(crate) enum LockError {
    /// Another process held a conflicting lock for longer than the time
    /// allowed.
    Busy,
    /// The operating system failed the call. The kind is
    /// [`io::ErrorKind::Unsupported`] when the file system has no working
    /// byte-range locks.
    Io(io::Error),
}

/// This process's hold on one database file: its handles and its locks.
#[derive(Debug)]
pub(crate) struct Locks {
    /// The file the locks are on, and any other handle to it this process
    /// opened. `None` for a simulated disk, which no other process can reach.
    handles: Option<Mutex<Vec<Arc<DbFile>>>>,
    /// Whether this process holds the open lock.
    open: Mutex<bool>,
    /// The snapshots in use in this process, with the number of read
    /// transactions on each. The first registration of a snapshot takes a
    /// shared lock on its byte, and the last one to end releases it.
    snapshots: Mutex<BTreeMap<u64, usize>>,
    /// The process that opened the file. A process forked from it inherits
    /// this value, but not the locks.
    owner: u32,
}

impl Locks {
    /// The locks on `file`, none of them taken yet.
    pub(crate) fn on(file: Arc<DbFile>) -> Self {
        Self {
            handles: Some(Mutex::new(vec![file])),
            open: Mutex::new(false),
            snapshots: Mutex::new(BTreeMap::new()),
            owner: std::process::id(),
        }
    }

    /// Locks that always succeed, for a simulated disk.
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            handles: None,
            open: Mutex::new(false),
            snapshots: Mutex::new(BTreeMap::new()),
            owner: std::process::id(),
        }
    }

    /// Whether this is a process forked from the one that opened the file,
    /// which holds none of its locks.
    pub(crate) fn inherited(&self) -> bool {
        std::process::id() != self.owner
    }

    /// Keeps `file`, another handle to the same file, open until the locks are
    /// dropped. Closing it any sooner would release every lock the process
    /// holds on the file.
    pub(crate) fn keep(&self, file: Arc<DbFile>) {
        if let Some(handles) = &self.handles {
            lock(handles).push(file);
        }
    }

    /// Takes the open lock: exclusively if no other process has the file open,
    /// and otherwise shared, once any recovery in progress has finished. The
    /// wait for the shared lock lasts up to `timeout`.
    pub(crate) fn open(&self, timeout: Duration) -> Result<Access, LockError> {
        if self
            .try_lock(OPEN_BYTE, 1, Mode::Exclusive)
            .map_err(LockError::Io)?
        {
            *lock(&self.open) = true;

            return Ok(Access::Alone);
        }

        let deadline = Instant::now().checked_add(timeout);

        poll(deadline, || {
            self.try_lock(OPEN_BYTE, 1, Mode::Shared)
                .map_err(LockError::Io)
        })?;
        *lock(&self.open) = true;

        Ok(Access::Shared)
    }

    /// Converts the open lock, held exclusively, to shared, letting other
    /// processes open the file.
    ///
    /// No other process can slip in between: a Unix-like system converts a
    /// record lock in place, and on Windows the shared lock is taken while the
    /// exclusive one is still held, after which one unlock releases the
    /// exclusive one.
    ///
    /// Should Windows refuse the shared lock alongside the exclusive one, the
    /// exclusive lock is released first and the shared one waited for, up to
    /// `timeout`. The gap that leaves is harmless here, because this process
    /// has finished recovery and begun nothing yet: another process that
    /// recovers the file meanwhile finds nothing to change.
    pub(crate) fn share(&self, timeout: Duration) -> Result<(), LockError> {
        let shared = self
            .try_lock(OPEN_BYTE, 1, Mode::Shared)
            .map_err(LockError::Io)?;

        if cfg!(windows) || !shared {
            self.unlock(OPEN_BYTE, 1).map_err(LockError::Io)?;
        }

        if !shared {
            let deadline = Instant::now().checked_add(timeout);

            poll(deadline, || {
                self.try_lock(OPEN_BYTE, 1, Mode::Shared)
                    .map_err(LockError::Io)
            })?;
        }

        Ok(())
    }

    /// Takes the open lock exclusively, waiting up to `timeout` for every other
    /// process to close the file. Creating a database where the file system
    /// has no links needs it, to write the first page before anyone reads it.
    pub(crate) fn open_alone(&self, timeout: Duration) -> Result<(), LockError> {
        let deadline = Instant::now().checked_add(timeout);

        poll(deadline, || {
            self.try_lock(OPEN_BYTE, 1, Mode::Exclusive)
                .map_err(LockError::Io)
        })?;
        *lock(&self.open) = true;

        Ok(())
    }

    /// Takes the writer lock, waiting up to `deadline` for another process's
    /// write transaction to end. The attempt is repeated rather than blocking,
    /// so that the wait has a limit on every platform.
    pub(crate) fn lock_writer(&self, deadline: Option<Instant>) -> Result<(), LockError> {
        poll(deadline, || {
            self.try_lock(WRITER_BYTE, 1, Mode::Exclusive)
                .map_err(LockError::Io)
        })
    }

    /// Releases the writer lock.
    pub(crate) fn unlock_writer(&self) {
        // If this fails, the lock goes when the file closes, and until then
        // other processes wait for it and fail with `BUSY`. There is nothing
        // better to do with the failure.
        let _ = self.unlock(WRITER_BYTE, 1);
    }

    /// Registers a read transaction on snapshot `txn`, taking the snapshot's
    /// lock if no other read transaction in this process holds it already.
    ///
    /// The lock is shared and no one else takes it exclusively, apart from a
    /// writer probing the snapshot bytes on Windows for an instant; this waits
    /// that out, up to `deadline`.
    pub(crate) fn register(&self, txn: u64, deadline: Option<Instant>) -> Result<(), LockError> {
        let byte = snapshot_byte(txn)?;
        let mut snapshots = lock(&self.snapshots);

        if let Some(count) = snapshots.get_mut(&txn) {
            *count += 1;

            return Ok(());
        }

        poll(deadline, || {
            self.try_lock(byte, 1, Mode::Shared).map_err(LockError::Io)
        })?;
        snapshots.insert(txn, 1);

        Ok(())
    }

    /// Ends a read transaction on snapshot `txn`, releasing the snapshot's
    /// lock if it was the last one in this process.
    pub(crate) fn unregister(&self, txn: u64) {
        let mut snapshots = lock(&self.snapshots);

        if let Some(count) = snapshots.get_mut(&txn) {
            *count -= 1;

            if *count == 0 {
                snapshots.remove(&txn);

                // A lock left behind holds pages back from reuse, and
                // nothing more; it goes when the file closes.
                if let Ok(byte) = snapshot_byte(txn) {
                    let _ = self.unlock(byte, 1);
                }
            }
        }
    }

    /// The oldest snapshot a read transaction in this process uses.
    pub(crate) fn oldest_local(&self) -> Option<u64> {
        lock(&self.snapshots).keys().next().copied()
    }

    /// The largest of `groups`, retained group ids in ascending order, that no
    /// registered snapshot in any process can reach. A snapshot `s` reaches
    /// group `F` only if `s < F`.
    ///
    /// This process's snapshots rule out the groups above the oldest of them
    /// without a call to the operating system. For the rest, one question
    /// usually settles it: whether any snapshot lies below the largest group
    /// left. Only when one does are the groups bisected with the same
    /// question (`design/locking.md`, "Finding the oldest snapshot").
    ///
    /// The caller holds the writer lock. A snapshot registered from here on is
    /// at least the published commit, above every retained group, so the
    /// answers do not change while it looks.
    pub(crate) fn reclaimable(&self, groups: &[u64]) -> Result<Option<u64>, LockError> {
        let groups = match self.oldest_local() {
            Some(oldest) => &groups[..groups.partition_point(|group| *group <= oldest)],
            None => groups,
        };

        last_unreached(groups, |group| self.snapshot_below(group))
    }

    /// Whether a read transaction in any process uses a snapshot below `txn`.
    ///
    /// One call into the operating system answers for every other process:
    /// whether any lock lies on the snapshot bytes below `txn`'s. It cannot
    /// say which, which is why [`reclaimable`](Self::reclaimable) bisects with
    /// this question rather than asking for the oldest snapshot.
    pub(crate) fn snapshot_below(&self, txn: u64) -> Result<bool, LockError> {
        if txn == 0 {
            return Ok(false);
        }

        if self.oldest_local().is_some_and(|oldest| oldest < txn) {
            return Ok(true);
        }

        snapshot_byte(txn)?;
        self.is_locked(SNAPSHOT_BASE, txn).map_err(LockError::Io)
    }

    /// Takes a lock without waiting; `true` if it was granted.
    fn try_lock(&self, start: u64, len: u64, mode: Mode) -> io::Result<bool> {
        match &self.handles {
            None => Ok(true),
            Some(handles) => sys::try_lock(lock(handles)[0].as_file(), start, len, mode),
        }
    }

    fn is_locked(&self, start: u64, len: u64) -> io::Result<bool> {
        match &self.handles {
            None => Ok(false),
            Some(handles) => sys::is_locked(lock(handles)[0].as_file(), start, len),
        }
    }

    /// Releases a lock. In a forked process it does nothing: the lock was the
    /// parent's, and on a Unix-like system unlocking the same bytes would
    /// release a lock the child took on them itself, through its own handle,
    /// as when it drops an inherited read transaction on the snapshot it also
    /// reads.
    fn unlock(&self, start: u64, len: u64) -> io::Result<()> {
        if self.inherited() {
            return Ok(());
        }

        match &self.handles {
            None => Ok(()),
            Some(handles) => sys::unlock(lock(handles)[0].as_file(), start, len),
        }
    }
}

impl Drop for Locks {
    /// Releases the open lock. The operating system would release it when the
    /// handle closes; Windows asks for it to be done explicitly, because it
    /// may take its time otherwise.
    ///
    /// In a forked process nothing is released and no handle is closed: the
    /// locks are the parent's, and closing an inherited descriptor would
    /// release the locks this process took on the file itself since.
    fn drop(&mut self) {
        if self.inherited() {
            if let Some(handles) = self.handles.take() {
                std::mem::forget(handles);
            }

            return;
        }

        let snapshots = std::mem::take(&mut *lock(&self.snapshots));

        for txn in snapshots.keys() {
            if let Ok(byte) = snapshot_byte(*txn) {
                let _ = self.unlock(byte, 1);
            }
        }

        if std::mem::take(&mut *lock(&self.open)) {
            let _ = self.unlock(OPEN_BYTE, 1);
        }
    }
}

/// The largest of `groups`, in ascending order, for which `below` says no
/// snapshot lies below it. `below` only ever turns from `false` to `true` as
/// the group grows, which is what lets one question about the largest group
/// settle the common case and bisection settle the rest.
fn last_unreached<E>(
    groups: &[u64],
    mut below: impl FnMut(u64) -> Result<bool, E>,
) -> Result<Option<u64>, E> {
    let Some(&largest) = groups.last() else {
        return Ok(None);
    };

    if !below(largest)? {
        return Ok(Some(largest));
    }

    // The first group some snapshot reaches lies in `low..=high`.
    let (mut low, mut high) = (0, groups.len() - 1);

    while low < high {
        let middle = low + (high - low) / 2;

        if below(groups[middle])? {
            high = middle;
        } else {
            low = middle + 1;
        }
    }

    Ok(low.checked_sub(1).map(|index| groups[index]))
}

/// The lock byte of snapshot `txn`.
fn snapshot_byte(txn: u64) -> Result<u64, LockError> {
    if txn >= TXN_LIMIT {
        return Err(LockError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a transaction id past the last one a file may use",
        )));
    }

    Ok(SNAPSHOT_BASE + txn)
}

/// Runs `attempt` until it succeeds or `deadline` passes, pausing a little
/// longer after each failure. No deadline waits for ever.
fn poll(
    deadline: Option<Instant>,
    mut attempt: impl FnMut() -> Result<bool, LockError>,
) -> Result<(), LockError> {
    let mut pause = FIRST_PAUSE;

    loop {
        if attempt()? {
            return Ok(());
        }

        let wait = match deadline {
            None => pause,
            Some(deadline) => {
                let now = Instant::now();

                if now >= deadline {
                    return Err(LockError::Busy);
                }

                pause.min(deadline - now)
            }
        };

        thread::sleep(wait);
        pause = (pause * 2).min(LAST_PAUSE);
    }
}

/// Locks a mutex, carrying on if a thread panicked while holding it: the
/// values behind these locks are left consistent between statements.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
