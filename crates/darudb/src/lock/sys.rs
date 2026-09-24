//! The operating system's byte-range locks, called directly: `fcntl` record
//! locks on Unix-like systems, `LockFileEx` and `UnlockFileEx` on Windows.
//!
//! This is the one module of the engine allowed `unsafe` code. The standard
//! library locks only whole files, and the crates that wrap byte-range locks
//! either lock whole files too or limit offsets to 32 bits, which the lock
//! bytes at 2^62 do not fit. Every `unsafe` block here is one call into the C
//! library or the Windows API, and its `SAFETY` comment says why what it passes
//! is valid. No pointer outlives the call it is passed to.
//!
//! Nothing here waits. A lock is granted at once or refused, and the callers
//! poll, which is what gives every wait a timeout on every platform. It also
//! keeps a waiting thread out of the Windows API, where a call that blocks on a
//! synchronous handle holds up every other call on that handle.

#![allow(unsafe_code)]

use std::fs::File;
use std::io;

/// What a lock admits alongside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// Other shared locks on the same bytes.
    Shared,
    /// No other lock on the same bytes.
    Exclusive,
}

/// Takes a lock of `mode` on the `len` bytes from `start`, without waiting.
///
/// Returns whether it was granted: `false` when another process holds a lock
/// that conflicts with it, and on Windows another handle too. A process never
/// conflicts with its own locks on Unix-like systems, where taking a lock over
/// one it holds converts it in place. A file system without byte-range locks
/// fails with [`io::ErrorKind::Unsupported`].
pub(super) fn try_lock(file: &File, start: u64, len: u64, mode: Mode) -> io::Result<bool> {
    platform::try_lock(file, start, len, mode)
}

/// Whether some lock on the `len` bytes from `start` would stop an exclusive
/// lock there: one another process holds, on Unix-like systems, and one any
/// handle holds, this one included, on Windows.
///
/// Unix-like systems answer the question directly. Windows has no such query,
/// so the answer comes from taking an exclusive lock without waiting and
/// releasing it at once if it was granted; a reader asking for a lock in the
/// range meanwhile waits an instant.
pub(super) fn is_locked(file: &File, start: u64, len: u64) -> io::Result<bool> {
    platform::is_locked(file, start, len)
}

/// Releases the lock on the `len` bytes from `start`. On Windows the range has
/// to be exactly one that was locked, and a range locked twice, shared and
/// exclusive, loses the exclusive lock first.
pub(super) fn unlock(file: &File, start: u64, len: u64) -> io::Result<()> {
    platform::unlock(file, start, len)
}

/// The error for a file system whose locks do not work.
fn unsupported(source: io::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, source)
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::mem;
    use std::os::fd::AsRawFd;

    use super::{Mode, unsupported};

    /// The lock record and the command that sets it. On 32-bit Linux and
    /// Android, `off_t` is 32 bits wide, so the lock bytes at 2^62 need the
    /// 64-bit record and command, which `libc` does not define for them. The
    /// numbers are the kernel's, from `asm-generic/fcntl.h`, and MIPS has its
    /// own.
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        target_pointer_width = "32",
        not(target_env = "musl")
    ))]
    mod calls {
        pub(super) type Record = libc::flock64;

        #[cfg(any(target_arch = "mips", target_arch = "mips32r6"))]
        pub(super) const GET: libc::c_int = 33;

        #[cfg(any(target_arch = "mips", target_arch = "mips32r6"))]
        pub(super) const SET: libc::c_int = 34;

        #[cfg(not(any(target_arch = "mips", target_arch = "mips32r6")))]
        pub(super) const GET: libc::c_int = 12;

        #[cfg(not(any(target_arch = "mips", target_arch = "mips32r6")))]
        pub(super) const SET: libc::c_int = 13;
    }

    /// Every other Unix-like target has a 64-bit `off_t`, musl included.
    #[cfg(not(all(
        any(target_os = "linux", target_os = "android"),
        target_pointer_width = "32",
        not(target_env = "musl")
    )))]
    mod calls {
        pub(super) type Record = libc::flock;

        pub(super) const GET: libc::c_int = libc::F_GETLK;

        pub(super) const SET: libc::c_int = libc::F_SETLK;
    }

    /// A lock record of type `kind` over the `len` bytes from `start`. The lock
    /// types are `short` on some systems and `int` on others.
    fn record(
        kind: impl TryInto<libc::c_short>,
        start: u64,
        len: u64,
    ) -> io::Result<calls::Record> {
        let offset = |value: u64| {
            i64::try_from(value).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a lock range past the largest file offset",
                )
            })
        };
        // SAFETY: the lock record is a plain C struct of integers, for which
        // all zeros is a valid value; the fields that matter are set below.
        let mut record: calls::Record = unsafe { mem::zeroed() };

        record.l_type = narrow(kind)?;
        record.l_whence = narrow(libc::SEEK_SET)?;
        record.l_start = offset(start)?;
        record.l_len = offset(len)?;

        Ok(record)
    }

    /// A lock type or origin in the `short` field the record keeps it in.
    fn narrow(value: impl TryInto<libc::c_short>) -> io::Result<libc::c_short> {
        value
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a lock constant too wide"))
    }

    /// Runs `command` on `record`, retrying when a signal interrupts it.
    fn fcntl(file: &File, command: libc::c_int, record: &mut calls::Record) -> io::Result<()> {
        loop {
            // SAFETY: the descriptor belongs to `file`, which is open for the
            // whole call, and `record` is a valid, exclusively borrowed lock
            // record of the type `command` expects.
            let result = unsafe { libc::fcntl(file.as_raw_fd(), command, &raw mut *record) };

            if result != -1 {
                return Ok(());
            }

            let error = io::Error::last_os_error();

            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    /// Whether `error` says the file system does not support record locks:
    /// a network file system without a lock service, or a user-space one that
    /// does not implement them.
    fn is_unsupported(error: &io::Error) -> bool {
        // `ENOTSUP` and `EOPNOTSUPP` are one number on some systems and two on
        // others, so they are compared rather than matched.
        error.raw_os_error().is_some_and(|code| {
            [libc::ENOLCK, libc::ENOSYS, libc::EOPNOTSUPP, libc::ENOTSUP].contains(&code)
        })
    }

    pub(super) fn try_lock(file: &File, start: u64, len: u64, mode: Mode) -> io::Result<bool> {
        let kind = match mode {
            Mode::Shared => libc::F_RDLCK,
            Mode::Exclusive => libc::F_WRLCK,
        };
        let mut record = record(kind, start, len)?;

        match fcntl(file, calls::SET, &mut record) {
            Ok(()) => Ok(true),
            // POSIX allows either for a lock another process holds.
            Err(error) if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EAGAIN)) => {
                Ok(false)
            }
            Err(error) if is_unsupported(&error) => Err(unsupported(error)),
            Err(error) => Err(error),
        }
    }

    pub(super) fn is_locked(file: &File, start: u64, len: u64) -> io::Result<bool> {
        let mut record = record(libc::F_WRLCK, start, len)?;

        // The call leaves the record alone when nothing is in the way, and
        // describes the first lock that is otherwise.
        match fcntl(file, calls::GET, &mut record) {
            Ok(()) => Ok(record.l_type != narrow(libc::F_UNLCK)?),
            Err(error) if is_unsupported(&error) => Err(unsupported(error)),
            Err(error) => Err(error),
        }
    }

    pub(super) fn unlock(file: &File, start: u64, len: u64) -> io::Result<()> {
        let mut record = record(libc::F_UNLCK, start, len)?;

        fcntl(file, calls::SET, &mut record)
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{
        ERROR_INVALID_FUNCTION, ERROR_LOCK_VIOLATION, ERROR_NOT_SUPPORTED,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
    };
    use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};

    use super::{Mode, unsupported};

    /// The offset of a lock, where the Windows API takes it.
    fn at(start: u64) -> OVERLAPPED {
        let (low, high) = halves(start);

        OVERLAPPED {
            Anonymous: OVERLAPPED_0 {
                Anonymous: OVERLAPPED_0_0 {
                    Offset: low,
                    OffsetHigh: high,
                },
            },
            ..OVERLAPPED::default()
        }
    }

    /// The low and the high 32 bits of `value`.
    fn halves(value: u64) -> (u32, u32) {
        let [a, b, c, d, e, f, g, h] = value.to_le_bytes();

        (
            u32::from_le_bytes([a, b, c, d]),
            u32::from_le_bytes([e, f, g, h]),
        )
    }

    pub(super) fn try_lock(file: &File, start: u64, len: u64, mode: Mode) -> io::Result<bool> {
        let (low, high) = halves(len);
        let mut overlapped = at(start);
        let flags = match mode {
            Mode::Shared => LOCKFILE_FAIL_IMMEDIATELY,
            Mode::Exclusive => LOCKFILE_FAIL_IMMEDIATELY | LOCKFILE_EXCLUSIVE_LOCK,
        };
        // SAFETY: the handle belongs to `file`, which is open for the whole
        // call. It was opened for synchronous I/O, so the call returns before
        // `overlapped`, which only carries the offset, goes out of scope.
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                flags,
                0,
                low,
                high,
                &raw mut overlapped,
            )
        };

        if locked != 0 {
            return Ok(true);
        }

        let error = io::Error::last_os_error();

        match error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
        {
            Some(ERROR_LOCK_VIOLATION) => Ok(false),
            Some(ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION) => Err(unsupported(error)),
            _ => Err(error),
        }
    }

    pub(super) fn is_locked(file: &File, start: u64, len: u64) -> io::Result<bool> {
        if !try_lock(file, start, len, Mode::Exclusive)? {
            return Ok(true);
        }

        unlock(file, start, len)?;

        Ok(false)
    }

    pub(super) fn unlock(file: &File, start: u64, len: u64) -> io::Result<()> {
        let (low, high) = halves(len);
        let mut overlapped = at(start);
        // SAFETY: as in `try_lock`: an open handle, and an offset record that
        // lives past the synchronous call.
        let unlocked =
            unsafe { UnlockFileEx(file.as_raw_handle(), 0, low, high, &raw mut overlapped) };

        if unlocked == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }
}
