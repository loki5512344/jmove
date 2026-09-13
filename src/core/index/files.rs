//! Indexed source files with O(1) membership lookups.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The set of project-relative source files discovered by [`Index::build`](super::Index::build).
#[derive(Debug, Default)]
pub struct FileSet {
    paths: HashSet<PathBuf>,
}

impl FileSet {
    /// Add a normalized project-relative path; `false` if already present.
    pub fn add(&mut self, path: PathBuf) -> bool {
        self.paths.insert(path)
    }

    /// Whether `path` is a known indexed source file.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    /// All files in deterministic sorted order (stable for tests and diffs).
    #[must_use]
    pub fn sorted(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.paths.iter().cloned().collect();
        v.sort();
        v
    }
}
