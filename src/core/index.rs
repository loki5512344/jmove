//! Project indexing: gitignore-aware filesystem scan plus the import graph.
//!
//! Built fresh on every command (disk cache is Phase 2). `ignore::WalkBuilder`
//! handles `.gitignore`/hidden-file rules; every indexed TS/JS source file is
//! parsed through [`crate::parser`] and its specifiers resolved through
//! [`crate::parser::resolve`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::core::JmoveResult;
use crate::parser::ImportRecord;

/// Indexed source files with O(1) membership lookups.
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

/// One import occurrence plus the project file it resolves to.
/// `target: None` means "external" — a bare package specifier or a path
/// that does not exist in the index.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// Raw record from the parser (specifier text + byte span).
    pub record: ImportRecord,
    /// Project-relative resolved file, if any.
    pub target: Option<PathBuf>,
}

/// Full in-memory project index: file set and forward import edges.
#[derive(Debug, Default)]
pub struct Index {
    /// Absolute project root the index was built for.
    pub root: PathBuf,
    /// All indexed source files.
    pub files: FileSet,
    /// For each file, the imports it declares (in source order).
    pub imports: HashMap<PathBuf, Vec<ResolvedImport>>,
}

impl Index {
    /// Scan `root`, parse every supported source file and build the graph.
    /// Unparseable files are skipped, not fatal.
    pub fn build(root: &Path) -> JmoveResult<Self> {
        let _ = root;
        todo!(
            "index agent: scan with `ignore`, parse via crate::parser, resolve via parser::resolve"
        )
    }

    /// Reverse edge lookup: every indexed file that imports `target`.
    #[must_use]
    pub fn importers_of(&self, target: &Path) -> Vec<PathBuf> {
        let _ = target;
        todo!("index agent: reverse-edge lookup")
    }
}
