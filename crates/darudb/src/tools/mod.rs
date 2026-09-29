//! The tools that ship with the library, for a file that has to be checked,
//! copied, made smaller or rescued (`design/tools.md`).
//!
//! Each works on the commit a read transaction sees, or on a new file it
//! writes, so none of them stops other handles and processes from using the
//! file while it runs.

mod backup;
mod check;
mod compact;

pub use backup::BackupReport;
pub use check::{CheckReport, Problem};
pub use compact::CompactReport;

pub(crate) use backup::backup;
pub(crate) use check::check;
pub(crate) use compact::compact;
