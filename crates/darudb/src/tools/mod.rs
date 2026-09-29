//! The tools that ship with the library, for a file that has to be checked,
//! copied, made smaller or rescued (`design/tools.md`).
//!
//! Each works on the commit a read transaction sees, or on a new file it
//! writes, so none of them stops other handles and processes from using the
//! file while it runs.

mod check;

pub use check::{CheckReport, Problem};

pub(crate) use check::check;
