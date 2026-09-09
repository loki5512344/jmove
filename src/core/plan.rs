//! Move planning: decide which import specifiers must be rewritten.
//!
//! A plan is pure data (no disk writes), so dry-run and `--json` can render
//! it without touching the filesystem.

use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use crate::core::JmoveResult;
use crate::core::index::Index;

/// One in-file replacement of an import specifier. Only the specifier text
/// between the quotes is touched — the statement layout is never reformatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// Project-relative file to modify.
    pub file: PathBuf,
    /// Byte range of the old specifier text (without quotes).
    pub span: Range<usize>,
    /// Specifier as currently written.
    pub old_text: String,
    /// Specifier after the move.
    pub new_text: String,
}

/// Complete plan for moving `source` to `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    /// Project-relative path being moved.
    pub source: PathBuf,
    /// Project-relative destination path.
    pub target: PathBuf,
    /// Specifier rewrites, grouped per importer file.
    pub rewrites: Vec<Rewrite>,
}

/// Compute the rewrite plan for `source -> target`.
///
/// Every indexed import whose resolved target is `source` gets a new
/// relative specifier computed from the *importer's* directory to `target`.
/// Rewrites whose result equals the old specifier are dropped.
pub fn plan_move(index: &Index, source: &Path, target: &Path) -> JmoveResult<MovePlan> {
    let _ = (index, source, target);
    todo!("index agent: implement planner incl. relative-specifier math")
}
