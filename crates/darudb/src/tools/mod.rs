//! The tools that ship with the library, for a file that has to be checked,
//! copied, made smaller or rescued (`design/tools.md`).
//!
//! Each works on the commit a read transaction sees, or on a new file it
//! writes, so none of them stops other handles and processes from using the
//! file while it runs. Salvage is the exception: it reads a file that may
//! not open, and needs it alone.

mod backup;
mod check;
mod compact;
mod salvage;

pub use backup::BackupReport;
pub use check::{CheckReport, Problem};
pub use compact::CompactReport;
pub use salvage::SalvageReport;

pub(crate) use backup::backup;
pub(crate) use check::check;
pub(crate) use compact::compact;
pub(crate) use salvage::salvage;

use std::fs;
use std::path::{Path, PathBuf};

use crate::database::Database;
use crate::error::{Error, Result};
use crate::storage::move_into_place;

/// How many bytes of keys and values a write transaction of a new file
/// holds before it commits and a new one begins: the pages it changes stay
/// in memory until then. The tests write with less, so that a file of a few
/// thousand entries takes several commits.
const COMMIT_BYTES: usize = if cfg!(test) { 64 << 10 } else { 32 << 20 };

/// Fills `copy`, a new database under the temporary name `temporary` beside
/// `path`, with `fill`, closes it, and moves it to `path`. The temporary file
/// goes if anything fails, and a file that took `path` meanwhile is never
/// replaced: `tool`, as in "a backup", names what refused to in the error.
/// Returns what `fill` returned and the size of the new file.
fn write_new<R>(
    path: &Path,
    tool: &str,
    (temporary, copy): (PathBuf, Database),
    fill: impl FnOnce(&Database) -> Result<R>,
) -> Result<(R, u64)> {
    let written = fill(&copy)
        .and_then(|filled| copy.close().map(|()| filled))
        .and_then(|filled| {
            fs::metadata(&temporary)
                .map(|metadata| (filled, metadata.len()))
                .map_err(|source| io_error(&temporary, source))
        });
    let written = match written {
        Ok(written) => written,
        Err(error) => {
            let _ = fs::remove_file(&temporary);

            return Err(error);
        }
    };

    move_into_place(&temporary, path).map_err(|source| match source.kind() {
        std::io::ErrorKind::AlreadyExists => taken(path, tool),
        _ => io_error(path, source),
    })?;

    Ok(written)
}

/// The error for a path a tool will not write over.
fn taken(path: &Path, tool: &str) -> Error {
    Error::InvalidArgument {
        message: format!(
            "`{}` exists already, and {tool} never replaces a file",
            path.display()
        ),
    }
}

fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}
