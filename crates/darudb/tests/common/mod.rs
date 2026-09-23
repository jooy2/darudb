//! What every integration test shares.
//!
//! Each test gets a directory of its own, removed when the test ends, so tests
//! can run in parallel and never find a file another test left behind.

// Not every test file uses every helper.
#![allow(dead_code)]
// A helper that cannot do its job fails the test that called it, which is what
// `expect` does. `clippy.toml` allows it inside tests but cannot tell that these
// functions only ever run inside one.
#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// A temporary directory for one test, deleted when it goes out of scope.
pub(crate) struct TestDir {
    dir: TempDir,
}

impl TestDir {
    pub(crate) fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("a temporary directory can be created"),
        }
    }

    /// A path inside the directory. Nothing is created there.
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Writes `bytes` to a file in the directory and returns its path.
    pub(crate) fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(name);

        fs::write(&path, bytes).expect("a test file can be written");

        path
    }
}

/// The bytes of the file at `path`.
pub(crate) fn read(path: &Path) -> Vec<u8> {
    fs::read(path).expect("the test file can be read")
}

/// Overwrites the bytes of the file at `path` from `offset` on, the way a
/// damaged disk or another program would.
pub(crate) fn patch(path: &Path, offset: usize, bytes: &[u8]) {
    let mut contents = read(path);

    contents[offset..offset + bytes.len()].copy_from_slice(bytes);
    fs::write(path, contents).expect("the test file can be written");
}
