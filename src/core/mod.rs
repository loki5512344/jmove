//! Core engine: project indexing, dependency graph, move planning and
//! atomic apply with rollback.
//!
//! Path convention used across the crate: every `PathBuf` produced by
//! `jmove` is **project-root-relative, normalized** (no `.`/`..` segments).
//! Use [`normalize_rel_path`] to canonicalize paths coming from users or
//! from OS walking.

pub mod apply;
pub mod index;
pub mod plan;

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Crate-wide error type surfaced to the CLI layer.
#[derive(Debug, Error)]
pub enum JmoveError {
    /// Filesystem or IO failure.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The user supplied a path that is invalid for the requested operation.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// Project index is stale (a file vanished or was moved externally).
    #[error("index is stale: {0}")]
    StaleIndex(String),
    /// The planned move cannot be applied safely.
    #[error("plan rejected: {0}")]
    PlanRejected(String),
}

/// Result alias used throughout the crate.
pub type JmoveResult<T> = Result<T, JmoveError>;

/// Normalize a project-relative path: strip `.` segments, collapse `..`
/// where possible and reject paths that escape the project root.
/// Returns `None` if the result would be empty, absolute or above the root.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
/// use jmove::core::normalize_rel_path;
///
/// assert_eq!(
///     normalize_rel_path(Path::new("./src/../utils/foo.ts")),
///     Some(PathBuf::from("utils/foo.ts"))
/// );
/// assert_eq!(normalize_rel_path(Path::new("../outside")), None);
/// ```
#[must_use]
pub fn normalize_rel_path(path: &Path) -> Option<PathBuf> {
    let mut stack: Vec<&OsStr> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // `?` on the popped Option: escaping the root yields None.
                stack.pop()?;
            }
            Component::Normal(piece) => stack.push(piece),
            // Absolute paths and Windows prefixes are not project-relative.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!stack.is_empty()).then(|| stack.iter().collect::<PathBuf>())
}

#[cfg(test)]
mod tests {
    use super::normalize_rel_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn normalizes_dots_and_parent_dirs() {
        assert_eq!(
            normalize_rel_path(Path::new("./src/../utils/foo.ts")),
            Some(PathBuf::from("utils/foo.ts"))
        );
        assert_eq!(
            normalize_rel_path(Path::new("a/b/c/../../d.ts")),
            Some(PathBuf::from("a/d.ts"))
        );
    }

    #[test]
    fn rejects_escaping_and_empty_paths() {
        assert_eq!(normalize_rel_path(Path::new("../outside")), None);
        assert_eq!(normalize_rel_path(Path::new("a/../../outside")), None);
        assert_eq!(normalize_rel_path(Path::new("")), None);
        assert_eq!(normalize_rel_path(Path::new("./")), None);
    }

    #[test]
    fn rejects_absolute_paths() {
        assert_eq!(normalize_rel_path(Path::new("/etc/passwd")), None);
    }
}
