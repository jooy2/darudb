//! The operating system's byte-range locks, called directly: `fcntl` record
//! locks on Unix-like systems, `LockFileEx` and `UnlockFileEx` on Windows.
//! Also the question those locks depend on: whether a file is on a network
//! file system, where they do not work.
//!
//! This is the one module of the engine allowed `unsafe` code. The standard
//! library locks only whole files, and the crates that wrap byte-range locks
//! either lock whole files too or limit offsets to 32 bits, which the lock
//! bytes at 2^62 do not fit. Every `unsafe` block here is one call into the C
//! library or the Windows API, or the zeroing of a C struct for one, and its
//! `SAFETY` comment says why that is sound. No pointer outlives the call it is
//! passed to.
//!
//! Nothing here waits. A lock is granted at once or refused, and the callers
//! poll, which is what gives every wait a timeout on every platform. It also
//! keeps a waiting thread out of the Windows API, where a call that blocks on a
//! synchronous handle holds up every other call on that handle.

#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::path::Path;

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

/// Whether the file system that holds `path` is a network one. `file` is the
/// file at `path` once it is open; before it exists, the directory it goes
/// into answers for it.
///
/// Linux and Android ask the file system for its type, macOS and iOS for its
/// name, and Windows looks for a UNC path or a drive the system reports as
/// remote. The answer is a best effort: a network file system these do not
/// recognise is still not supported, and a question the system fails to
/// answer counts as a local file system.
pub(super) fn is_remote(file: Option<&File>, path: &Path) -> bool {
    platform::is_remote(file, path).unwrap_or(false)
}

/// Forks this process, for the tests of what a forked child inherits. Returns
/// the child's process id in the parent, and `None` in the child.
#[cfg(all(test, unix))]
pub(super) fn fork() -> io::Result<Option<libc::pid_t>> {
    // SAFETY: only a helper process of the tests calls it, while its other
    // threads are idle, and the child only uses the database and then ends
    // with `exit_now`, without running the parent's destructors.
    match unsafe { libc::fork() } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(None),
        child => Ok(Some(child)),
    }
}

/// Waits for the forked process `child` and returns its exit code.
#[cfg(all(test, unix))]
pub(super) fn wait_for(child: libc::pid_t) -> io::Result<i32> {
    let mut status = 0;

    loop {
        // SAFETY: `status` is a valid, exclusively borrowed `int` for the call.
        if unsafe { libc::waitpid(child, &raw mut status, 0) } != -1 {
            break;
        }

        let error = io::Error::last_os_error();

        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }

    if libc::WIFEXITED(status) {
        Ok(libc::WEXITSTATUS(status))
    } else {
        Err(io::Error::other("the forked process did not exit normally"))
    }
}

/// Ends a forked child at once, with `code`, running none of the destructors
/// it inherited from its parent.
#[cfg(all(test, unix))]
pub(super) fn exit_now(code: i32) -> ! {
    // SAFETY: `_exit` takes any exit code and never returns.
    unsafe { libc::_exit(code) }
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
    /// numbers are the kernel's, from `asm-generic/fcntl.h`. x32 is not among
    /// these targets: its pointers are 32 bits wide, but its `off_t` and its
    /// system calls are the 64-bit ones.
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        target_pointer_width = "32",
        not(target_env = "musl"),
        not(target_arch = "x86_64")
    ))]
    mod calls {
        pub(super) type Record = libc::flock64;

        pub(super) const GET: libc::c_int = 12;

        pub(super) const SET: libc::c_int = 13;
    }

    // MIPS numbers the 64-bit commands differently, and `libc` has no 64-bit
    // lock record for it with glibc. Guessing at the record's layout in code
    // that nothing here can test would be worse than not building.
    #[cfg(all(
        target_os = "linux",
        target_pointer_width = "32",
        not(target_env = "musl"),
        any(target_arch = "mips", target_arch = "mips32r6")
    ))]
    compile_error!(
        "DaruDB does not build for 32-bit MIPS with glibc: the `libc` crate has no 64-bit lock record for it, which the lock bytes at 2^62 need"
    );

    /// Every other Unix-like target has a 64-bit `off_t`, musl and x32
    /// included.
    #[cfg(not(all(
        any(target_os = "linux", target_os = "android"),
        target_pointer_width = "32",
        not(target_env = "musl"),
        not(target_arch = "x86_64")
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

    #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
    pub(super) fn is_remote(file: Option<&File>, path: &std::path::Path) -> io::Result<bool> {
        let directory;
        let file = match file {
            Some(file) => file,
            None => {
                let parent = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(std::path::Path::new("."));

                directory = File::open(parent)?;

                &directory
            }
        };
        // SAFETY: `statfs` is a plain C struct of integers and characters, for
        // which all zeros is a valid value; the call below fills it in.
        let mut stat: libc::statfs = unsafe { mem::zeroed() };
        // SAFETY: the descriptor belongs to `file`, which is open for the
        // whole call, and `stat` is a valid, exclusively borrowed `statfs`.
        let result = unsafe { libc::fstatfs(file.as_raw_fd(), &raw mut stat) };

        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        let remote = super::is_network_type(stat.f_type);

        #[cfg(target_vendor = "apple")]
        let remote = super::is_network_name(
            &stat
                .f_fstypename
                .iter()
                .take_while(|character| **character != 0)
                .map(|character| character.to_ne_bytes()[0])
                .collect::<Vec<u8>>(),
        );

        Ok(remote)
    }

    /// Other Unix-like systems are not checked.
    #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
    pub(super) fn is_remote(_file: Option<&File>, _path: &std::path::Path) -> io::Result<bool> {
        Ok(false)
    }
}

/// Whether a Linux file system type, as `statfs` reports it, is a network
/// one: NFS, SMB in its three generations, NCP, Coda, and AFS in its two.
/// FUSE is left alone; a FUSE file system whose locks do not work, such as
/// shared storage on some Android devices, fails the lock call instead.
#[cfg(any(target_os = "linux", target_os = "android", test))]
pub(super) fn is_network_type(kind: impl Into<i128>) -> bool {
    const NETWORK: [u32; 8] = [
        0x0000_6969, // NFS
        0x0000_517B, // SMB
        0xFF53_4D42, // CIFS
        0xFE53_4D42, // SMB2 and later
        0x0000_564C, // NCP
        0x7375_7245, // Coda
        0x5346_414F, // AFS
        0x6B41_4653, // kAFS
    ];

    // The type is a 32-bit number, kept in a field whose width and sign vary
    // between systems.
    u32::try_from(kind.into() & 0xFFFF_FFFF).is_ok_and(|kind| NETWORK.contains(&kind))
}

/// Whether a file system name, as macOS and iOS report it, is a network one.
#[cfg(any(target_vendor = "apple", test))]
pub(super) fn is_network_name(name: &[u8]) -> bool {
    [b"nfs".as_slice(), b"smbfs", b"afpfs", b"webdav"].contains(&name)
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
        GetDriveTypeW, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
    };
    use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE;

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

    /// A UNC path is a network share, and so is a drive letter the system
    /// reports as remote, such as a mapped network drive.
    pub(super) fn is_remote(_file: Option<&File>, path: &std::path::Path) -> io::Result<bool> {
        use std::path::{Component, Prefix};

        let absolute = std::path::absolute(path)?;
        let Some(Component::Prefix(prefix)) = absolute.components().next() else {
            return Ok(false);
        };

        match prefix.kind() {
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => Ok(true),
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
                // SAFETY: `root` is a wide string ending in a zero, as the
                // call expects, and it lives past the call.
                let kind = unsafe { GetDriveTypeW(root.as_ptr()) };

                Ok(kind == DRIVE_REMOTE)
            }
            _ => Ok(false),
        }
    }
}
