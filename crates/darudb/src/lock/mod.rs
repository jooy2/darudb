//! Coordination between the processes that share one database file, through
//! the operating system's byte-range locks and nothing else.
//!
//! `design/locking.md` is the specification. The locks sit on bytes far past
//! the end of any data:
//!
//! | Byte            | Lock                                                        |
//! | --------------- | ----------------------------------------------------------- |
//! | 2^62            | Open: held for as long as the process has the file open     |
//! | 2^62 + 64 + `s` | Snapshot `s`: shared by every process reading that snapshot |
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
//! For now the open lock is held exclusively for as long as the file is open,
//! so a second process cannot open the file at all. The snapshot locks are
//! taken already, and the rest of the protocol, which lets a second process
//! in, is not in place yet.

mod sys;

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sys::Mode;

use crate::format::TXN_LIMIT;
use crate::storage::DbFile;

/// The open lock's byte, 2^62, the first of the lock bytes.
const OPEN_BYTE: u64 = 1 << 62;

/// The lock byte of snapshot 0. Snapshot `s` is locked at this byte plus `s`,
/// which stays below 2^63 because every transaction id is below
/// [`TXN_LIMIT`].
const SNAPSHOT_BASE: u64 = OPEN_BYTE + 64;

/// The first pause between two attempts at a lock another process holds.
const FIRST_PAUSE: Duration = Duration::from_micros(20);

/// The longest pause between two attempts: short enough to notice a released
/// lock soon, long enough not to keep a processor busy.
const LAST_PAUSE: Duration = Duration::from_millis(10);

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

    /// Takes the open lock exclusively, waiting up to `timeout` for every other
    /// process to close the file.
    pub(crate) fn open_alone(&self, timeout: Duration) -> Result<(), LockError> {
        let deadline = Instant::now().checked_add(timeout);

        poll(deadline, || {
            self.try_lock(OPEN_BYTE, 1, Mode::Exclusive)
                .map_err(LockError::Io)
        })?;
        *lock(&self.open) = true;

        Ok(())
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

    /// Takes a lock without waiting; `true` if it was granted.
    fn try_lock(&self, start: u64, len: u64, mode: Mode) -> io::Result<bool> {
        match &self.handles {
            None => Ok(true),
            Some(handles) => sys::try_lock(lock(handles)[0].as_file(), start, len, mode),
        }
    }

    fn unlock(&self, start: u64, len: u64) -> io::Result<()> {
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
