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
//! | 2^62 + 2        | Recovery: held by a process deciding whether to recover      |
//! | 2^62 + 3        | Turn: held by the waiting writer whose turn is next          |
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
//! to recover. Only a process that has finished opening holds it, shared, or a
//! process that is recovering the file, exclusively. A process on its way in
//! waits for the recovery lock instead, so that if the one recovering dies,
//! its open lock goes with it, and the next one in finds the file unopened and
//! recovers it.

mod sys;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sys::Mode;

use crate::format::TXN_LIMIT;
use crate::storage::DbFile;

/// The open lock's byte, 2^62, the first of the lock bytes.
const OPEN_BYTE: u64 = 1 << 62;

/// The writer lock's byte.
const WRITER_BYTE: u64 = OPEN_BYTE + 1;

/// The recovery lock's byte: held while a process opening the file decides
/// whether to recover it, and while it does.
const RECOVERY_BYTE: u64 = OPEN_BYTE + 2;

/// The turn lock's byte: held by a writer that has waited long enough to go
/// next, which every other writer lets go first.
const TURN_BYTE: u64 = OPEN_BYTE + 3;

/// How long a writer waits for the writer lock before it claims the turn.
/// Most waits are far shorter; this one is reached when a writer that commits
/// again and again keeps taking the lock back first.
///
/// Each time the writer lock passes to another process, the next commit
/// issues a barrier before its record, since it cannot know which selector a
/// power cut would bring back. A shorter wait would pass the lock back and
/// forth more often and pay that barrier each time: at 5 milliseconds, two
/// processes committing in tight loops made a twentieth of the commits they
/// made without a turn. At 50, the turn bounds the longest wait and costs
/// little throughput.
const TURN_AFTER: Duration = Duration::from_millis(50);

/// The longest pause between two attempts at the writer lock. Shorter than
/// [`LAST_PAUSE`], so that writers claiming the turn try as often as one
/// another.
const WRITER_PAUSE: Duration = Duration::from_millis(1);

/// The longest pause of the writer whose turn it is. Every other writer waits
/// for it, so the lock stays idle from its release until this writer tries
/// again, and that has to be short.
const TURN_PAUSE: Duration = Duration::from_micros(100);

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

/// A snapshot this process holds a lock on.
#[derive(Debug, Clone, Copy)]
struct Registration {
    /// The read transactions on the snapshot.
    readers: usize,
    /// Since when the lock has been kept with no reader, if it has.
    idle: Option<Instant>,
}

/// This process's registry of snapshots, locked for as long as this lives.
///
/// A reader reads the header while it holds this, so that a snapshot lock it
/// finds held was held throughout the read: every writer that could reclaim
/// the snapshot's pages starts after the read and finds the lock. Such a
/// reader needs neither a lock call nor a second read of the header.
#[derive(Debug)]
pub(crate) struct Registry<'a> {
    locks: &'a Locks,
    snapshots: MutexGuard<'a, BTreeMap<u64, Registration>>,
}

impl Registry<'_> {
    /// Registers a read transaction on snapshot `txn` if this process holds
    /// the snapshot's lock already, for another read transaction or kept
    /// after one; `false` if it does not.
    pub(crate) fn join(&mut self, txn: u64) -> bool {
        match self.snapshots.get_mut(&txn) {
            Some(registration) => {
                registration.readers += 1;
                registration.idle = None;

                true
            }
            None => false,
        }
    }

    /// Registers a read transaction on snapshot `txn`, which this process
    /// holds no lock on yet, and takes the lock.
    ///
    /// The lock is shared and no one else takes it exclusively, apart from a
    /// writer probing the snapshot bytes on Windows for an instant; this waits
    /// that out, up to `deadline`. The idle locks kept on older snapshots go:
    /// the published commit only grows, so no new read transaction can use
    /// them.
    pub(crate) fn register(
        &mut self,
        txn: u64,
        deadline: Option<Instant>,
    ) -> Result<(), LockError> {
        let byte = snapshot_byte(txn)?;

        poll(deadline, || {
            self.locks
                .try_lock(byte, 1, Mode::Shared)
                .map_err(LockError::Io)
        })?;
        self.snapshots.insert(
            txn,
            Registration {
                readers: 1,
                idle: None,
            },
        );

        let older: Vec<u64> = self
            .snapshots
            .range(..txn)
            .filter(|(_, registration)| registration.readers == 0)
            .map(|(older, _)| *older)
            .collect();

        for older in older {
            self.locks.release(&mut self.snapshots, older);
        }

        Ok(())
    }
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
    /// Whether this process holds the recovery lock.
    recovery: Mutex<bool>,
    /// The snapshots this process holds a lock on. The first registration of
    /// a snapshot takes a shared lock on its byte. When the last read
    /// transaction on it ends, the lock is kept for a moment, idle, in case the
    /// next read transaction is on the same snapshot.
    snapshots: Mutex<BTreeMap<u64, Registration>>,
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
            recovery: Mutex::new(false),
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
            recovery: Mutex::new(false),
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

    /// Takes the open lock, after the recovery lock, which it waits for up to
    /// `timeout`: exclusively if no other process has the file open, and
    /// otherwise shared.
    ///
    /// [`Access::Alone`] leaves the recovery lock held, so that no other
    /// process opens the file while this one recovers it; [`share`](Self::share)
    /// releases it. [`Access::Shared`] has released it already.
    pub(crate) fn open(&self, timeout: Duration) -> Result<Access, LockError> {
        let deadline = Instant::now().checked_add(timeout);

        self.lock_recovery(deadline)?;

        if self
            .try_lock(OPEN_BYTE, 1, Mode::Exclusive)
            .map_err(LockError::Io)?
        {
            *lock(&self.open) = true;

            return Ok(Access::Alone);
        }

        // The others hold it shared: a process holds it exclusively only while
        // it holds the recovery lock too. One running a build from before the
        // recovery lock may not, and is waited for.
        poll(deadline, || {
            self.try_lock(OPEN_BYTE, 1, Mode::Shared)
                .map_err(LockError::Io)
        })?;
        *lock(&self.open) = true;
        self.unlock_recovery();

        Ok(Access::Shared)
    }

    /// Takes the recovery lock, waiting up to `deadline` for another process
    /// that is opening the file or recovering it.
    fn lock_recovery(&self, deadline: Option<Instant>) -> Result<(), LockError> {
        poll(deadline, || {
            self.try_lock(RECOVERY_BYTE, 1, Mode::Exclusive)
                .map_err(LockError::Io)
        })?;
        *lock(&self.recovery) = true;

        Ok(())
    }

    /// Releases the recovery lock, if this process holds it.
    fn unlock_recovery(&self) {
        if std::mem::take(&mut *lock(&self.recovery)) {
            // If this fails, the lock goes when the file closes, and until
            // then other processes wait to open it and fail with `BUSY`.
            let _ = self.unlock(RECOVERY_BYTE, 1);
        }
    }

    /// Converts the open lock, held exclusively, to shared, and releases the
    /// recovery lock, letting other processes open the file.
    ///
    /// A Unix-like system converts a record lock in place, and on Windows the
    /// shared lock is taken while the exclusive one is still held, after which
    /// one unlock releases the exclusive one. Should Windows refuse the shared
    /// lock alongside the exclusive one, the exclusive lock is released first
    /// and the shared one waited for, up to `timeout`. No other process can
    /// open the file in that gap, since this one still holds the recovery
    /// lock.
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

        self.unlock_recovery();

        Ok(())
    }

    /// Takes the recovery lock and then the open lock exclusively, waiting up
    /// to `timeout` for every other process to finish opening the file or give
    /// up on it, and holds them as [`Access::Alone`] does. Creating a database
    /// where the file system has no links needs it, to write the first page
    /// before anyone reads it.
    pub(crate) fn open_alone(&self, timeout: Duration) -> Result<(), LockError> {
        let deadline = Instant::now().checked_add(timeout);

        self.lock_recovery(deadline)?;
        poll(deadline, || {
            self.try_lock(OPEN_BYTE, 1, Mode::Exclusive)
                .map_err(LockError::Io)
        })?;
        *lock(&self.open) = true;

        Ok(())
    }

    /// Takes the writer lock, waiting up to `deadline` for other processes'
    /// write transactions. The attempt is repeated rather than blocking, so
    /// that the wait has a limit on every platform.
    ///
    /// Record locks do not queue waiters, and a writer that commits again at
    /// once would take the lock back before a pausing one woke, again and
    /// again. The turn lock stops that. While another process holds it, this
    /// one leaves the writer lock alone; once this one has waited
    /// [`TURN_AFTER`], it claims the turn itself, if it is free, and keeps it
    /// until it has the writer lock.
    pub(crate) fn lock_writer(&self, deadline: Option<Instant>) -> Result<(), LockError> {
        let started = Instant::now();
        let mut turn = false;
        let mut pause = FIRST_PAUSE;
        let result = loop {
            match self.try_writer(started, &mut turn) {
                Ok(true) => break Ok(()),
                Ok(false) => {}
                Err(error) => break Err(error),
            }

            if let Err(error) = sleep_until(deadline, pause) {
                break Err(error);
            }

            pause = (pause * 2).min(if turn { TURN_PAUSE } else { WRITER_PAUSE });
        };

        if turn {
            let _ = self.unlock(TURN_BYTE, 1);
        }

        result
    }

    /// One attempt of [`lock_writer`](Self::lock_writer): `true` if it took
    /// the writer lock. `turn` says whether this process holds the turn lock,
    /// and the attempt claims it once the wait has lasted [`TURN_AFTER`].
    fn try_writer(&self, started: Instant, turn: &mut bool) -> Result<bool, LockError> {
        if !*turn && self.is_locked(TURN_BYTE, 1).map_err(LockError::Io)? {
            return Ok(false);
        }

        if self
            .try_lock(WRITER_BYTE, 1, Mode::Exclusive)
            .map_err(LockError::Io)?
        {
            return Ok(true);
        }

        if !*turn && started.elapsed() >= TURN_AFTER {
            *turn = self
                .try_lock(TURN_BYTE, 1, Mode::Exclusive)
                .map_err(LockError::Io)?;
        }

        Ok(false)
    }

    /// Releases the writer lock.
    pub(crate) fn unlock_writer(&self) {
        // If this fails, the lock goes when the file closes, and until then
        // other processes wait for it and fail with `BUSY`. There is nothing
        // better to do with the failure.
        let _ = self.unlock(WRITER_BYTE, 1);
    }

    /// Locks this process's registry of snapshots.
    pub(crate) fn registry(&self) -> Registry<'_> {
        Registry {
            locks: self,
            snapshots: lock(&self.snapshots),
        }
    }

    /// Ends a read transaction on snapshot `txn`. Returns whether that left
    /// the snapshot's lock idle, to be released by
    /// [`release_idle`](Self::release_idle) unless another read transaction
    /// takes it up first.
    pub(crate) fn unregister(&self, txn: u64) -> bool {
        let mut snapshots = lock(&self.snapshots);
        let Some(registration) = snapshots.get_mut(&txn) else {
            return false;
        };

        registration.readers -= 1;

        if registration.readers > 0 {
            return false;
        }

        #[cfg(test)]
        if !crate::testing::KEEP_SNAPSHOT_LOCKS.load(std::sync::atomic::Ordering::Relaxed) {
            self.release(&mut snapshots, txn);

            return false;
        }

        registration.idle = Some(Instant::now());

        true
    }

    /// Releases the idle snapshot locks kept since before `before`, or all of
    /// them with no time given. An idle lock holds pages back from reuse, in
    /// every process, so it is kept only briefly. Returns since when the
    /// oldest idle lock left has been kept, if any is.
    pub(crate) fn release_idle(&self, before: Option<Instant>) -> Option<Instant> {
        let mut snapshots = lock(&self.snapshots);
        let expired: Vec<u64> = snapshots
            .iter()
            .filter(|(_, registration)| {
                registration
                    .idle
                    .is_some_and(|idle| before.is_none_or(|before| idle < before))
            })
            .map(|(txn, _)| *txn)
            .collect();

        for txn in expired {
            self.release(&mut snapshots, txn);
        }

        snapshots
            .values()
            .filter_map(|registration| registration.idle)
            .min()
    }

    /// Forgets snapshot `txn` and releases its lock.
    fn release(&self, snapshots: &mut BTreeMap<u64, Registration>, txn: u64) {
        snapshots.remove(&txn);

        // A lock left behind holds pages back from reuse, and nothing more;
        // it goes when the file closes.
        if let Ok(byte) = snapshot_byte(txn) {
            let _ = self.unlock(byte, 1);
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
    /// The caller holds the writer lock, so a reader that registers from here
    /// on reads the published commit, above every retained group. One holding
    /// a stale header may register an older snapshot for a moment, but it then
    /// finds the snapshot no longer published and lets go of it without
    /// reading a page; the registration can only make the answers keep more
    /// pages, never fewer.
    pub(crate) fn reclaimable(&self, groups: &[u64]) -> Result<Option<u64>, LockError> {
        let groups = match self.oldest_local() {
            Some(oldest) => &groups[..groups.partition_point(|group| *group <= oldest)],
            None => groups,
        };

        last_unreached(groups, |group| self.snapshot_below(group))
    }

    /// The largest of `groups`, ids of commits after the durable commit
    /// `durable` in ascending order, whose young pages no registered snapshot
    /// in any process can reach. Young pages were written after `durable`, so
    /// a snapshot `s` reaches those of group `F` only if `durable < s < F`
    /// (`design/commits-and-recovery.md`, "Reclaiming pages"). Like
    /// [`reclaimable`](Self::reclaimable), it asks about the largest group
    /// first and bisects only when a snapshot lies below it.
    pub(crate) fn young_reclaimable(
        &self,
        durable: u64,
        groups: &[u64],
    ) -> Result<Option<u64>, LockError> {
        last_unreached(groups, |group| self.snapshot_between(durable, group))
    }

    /// Whether a read transaction in any process uses a snapshot `s` with
    /// `low < s < high`.
    fn snapshot_between(&self, low: u64, high: u64) -> Result<bool, LockError> {
        let first = low.saturating_add(1);

        if high <= first {
            return Ok(false);
        }

        if lock(&self.snapshots).range(first..high).next().is_some() {
            return Ok(true);
        }

        snapshot_byte(high)?;
        self.is_locked(snapshot_byte(first)?, high - first)
            .map_err(LockError::Io)
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

        // Still held only if opening failed while this process recovered.
        self.unlock_recovery();
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

        sleep_until(deadline, pause)?;
        pause = (pause * 2).min(LAST_PAUSE);
    }
}

/// Sleeps for `pause`, or until `deadline` if that comes first, and fails with
/// [`LockError::Busy`] once the deadline has passed. No deadline never fails.
fn sleep_until(deadline: Option<Instant>, pause: Duration) -> Result<(), LockError> {
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

    Ok(())
}

/// Locks a mutex, carrying on if a thread panicked while holding it: the
/// values behind these locks are left consistent between statements.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
