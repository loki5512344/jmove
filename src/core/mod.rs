//! Core engine: project indexing, dependency graph, move/fix planning and
//! atomic apply with rollback.
//!
//! Path convention used across the crate: every `PathBuf` produced by
//! `jmove` is **project-root-relative, normalized** (no `.`/`..` segments).
//! Use [`normalize_rel_path`] to canonicalize paths coming from users or
//! from OS walking.

pub mod apply;
pub mod fix;
pub mod index;
pub mod plan;
pub mod refs;

use std::ffi::OsStr;
use std::io;
use std::ops::Range;
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
    /// A git operation (`git mv`) failed.
    #[error("git error: {0}")]
    Git(String),
}

/// Result alias used throughout the crate.
pub type JmoveResult<T> = Result<T, JmoveError>;

/// One in-file byte-span edit: replaces the contents of `span` with
/// `new_text`, after verifying the bytes there still equal `old_text`.
///
/// This is the shared currency of every edit generator: `mv` produces
/// specifier replacements, the `fix` command will produce line deletions
/// and insertions. Engine semantics:
/// - **replace**: non-empty `span`, `old_text` is the current span content;
/// - **insert**: empty span (`start == end`) with empty `old_text` —
///   `new_text` lands at `span.start`;
/// - **delete**: empty `new_text`; to drop a whole line the generator
///   extends the span over its trailing `\n` and puts the exact line bytes
///   in `old_text` (the engine itself never grows spans).
///
/// The `old_text` check is the staleness guard: a plan whose file changed
/// since indexing is rejected before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Byte range in the original file contents.
    pub span: Range<usize>,
    /// Exact bytes the span must currently hold (empty for insertions).
    pub old_text: String,
    /// Replacement bytes (empty for deletions).
    pub new_text: String,
}

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

/// Format a project-relative path for anything that crosses the CLI
/// boundary (human messages, JSON payloads, diff headers, git pathspecs):
/// always `/`-separated, on every platform. `Path::display()` would leak
/// `\\` on Windows into texts where `/` is the contract — and git even
/// reads backslashes in pathspecs as escapes — so nothing user-facing
/// may use it for project-relative paths.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use jmove::core::rel_str;
///
/// assert_eq!(rel_str(Path::new("src/main/java/A.java")), "src/main/java/A.java");
/// ```
#[must_use]
pub fn rel_str(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// 1-based line containing the byte offset `byte` in `source`.
#[must_use]
pub fn line_of(source: &str, byte: usize) -> usize {
    let upto = source.len().min(byte);
    source.as_bytes()[..upto]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

/// Convert a user-supplied path to a normalized project-relative path.
/// Relative paths are taken against `root`; absolute ones must live
/// underneath it. Shared by CLI commands that accept paths.
///
/// # Errors
///
/// [`JmoveError::InvalidArgument`] when the path is outside `root` or
/// normalizes to nothing.
pub fn rel_from_root(root: &Path, path: &Path) -> JmoveResult<PathBuf> {
    let joined = if path.is_absolute() {
        path.into()
    } else {
        root.join(path)
    };
    let outside = || {
        JmoveError::InvalidArgument(format!(
            "path '{}' is outside the project root",
            path.display()
        ))
    };
    let abs = collapse(&joined);
    let rel = abs.strip_prefix(root).map_err(|_| outside())?;
    normalize_rel_path(rel).ok_or_else(|| {
        JmoveError::InvalidArgument(format!("invalid project path '{}'", path.display()))
    })
}

/// Lexically normalize a path: drop `.` segments, apply `..` where possible.
fn collapse(path: &Path) -> PathBuf {
    let mut stack: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.last() != Some(&Component::ParentDir) {
                    stack.pop();
                }
            }
            other => stack.push(other),
        }
    }
    stack.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{normalize_rel_path, rel_from_root, rel_str};
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

    #[test]
    fn rel_str_uses_forward_slashes() {
        assert_eq!(
            rel_str(Path::new("src/main/java/A.java")),
            "src/main/java/A.java"
        );
        assert_eq!(rel_str(Path::new("a.ts")), "a.ts");
        assert_eq!(rel_str(Path::new("")), "");
    }

    #[test]
    fn rel_from_root_resolves_inside_and_rejects_outside() {
        let root = Path::new("/tmp/proj");
        assert_eq!(
            rel_from_root(root, Path::new("src/a.ts")).unwrap(),
            PathBuf::from("src/a.ts")
        );
        assert_eq!(
            rel_from_root(root, Path::new("/tmp/proj/./src/../src/b.ts")).unwrap(),
            PathBuf::from("src/b.ts")
        );
        assert!(rel_from_root(root, Path::new("/elsewhere/x.ts")).is_err());
        assert!(rel_from_root(root, Path::new("../x.ts")).is_err());
    }
}
