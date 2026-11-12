//! Test helpers (`tempfile` is not a dependency of this crate).

use std::path::{Path, PathBuf};

/// Fresh directory under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let p = std::env::temp_dir().join(format!("stow-app-test-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
