//! Atomic apply with rollback, plus unified-diff rendering for dry-run.
//!
//! Order matters: specifier rewrites are applied to importer files first
//! (each written atomically via temp-file + rename), and the actual
//! `source -> target` rename happens last. Any failure mid-way triggers
//! rollback of everything already written.

use std::path::Path;

use crate::core::JmoveResult;
use crate::core::plan::MovePlan;

/// Summary of a successfully applied plan.
#[derive(Debug, Clone)]
pub struct Applied {
    /// Number of files whose imports were rewritten.
    pub files_rewritten: usize,
    /// The moved file's new project-relative path.
    pub new_path: std::path::PathBuf,
}

/// Apply `plan` under `root` atomically (see module docs). Rollback is
/// best-effort: on restore failure the error message states which files
/// need manual recovery.
pub fn apply(root: &Path, plan: &MovePlan) -> JmoveResult<Applied> {
    let _ = (root, plan);
    todo!("index agent: atomic apply + rollback")
}

/// Render the plan as a unified diff (rewrites + file rename) for dry-run.
#[must_use]
pub fn render_diff(root: &Path, plan: &MovePlan) -> JmoveResult<String> {
    let _ = (root, plan);
    todo!("index agent: diff rendering via `similar`")
}
