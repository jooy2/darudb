//! The operating system's calls that the standard library does not offer:
//! byte-range locks in [`lock`], and in [`fs`] a rename that never replaces a
//! file and, on Windows, what identifies a file.
//!
//! This is the one module of the engine allowed `unsafe` code, for those
//! calls and nothing else. Every `unsafe` block is one call into the C
//! library or the Windows API, or the zeroing of a C struct for one, and its
//! `SAFETY` comment says why that is sound. Keeping every such call here keeps
//! what a reviewer of `unsafe` has to read in one place.

#![allow(unsafe_code)]

pub(crate) mod fs;
pub(crate) mod lock;
