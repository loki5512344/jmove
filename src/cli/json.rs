//! JSON output shapes shared by all `--json` commands.
//!
//! Every response is an [`Envelope`] whose `status` is one of
//! `"ok" | "dry_run" | "error"`, plus a machine-readable error `code`
//! and a human `hint` on failure (see `docs/SKILL.md`). Payload structs
//! are flattened into the envelope, so they never repeat `operation`.

use serde::Serialize;

use crate::core::plan::MovePlan;
use crate::core::{JmoveError, rel_str};

use super::output;

/// Top-level envelope for every `--json` response.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    /// One of `"ok"`, `"dry_run"`, `"error"`.
    pub status: &'static str,
    /// The operation that produced this response, e.g. `"mv"`, `"check"`.
    pub operation: &'static str,
    /// Command-specific payload.
    #[serde(flatten)]
    pub data: T,
}

impl<T: Serialize> Envelope<T> {
    /// Success envelope: `status = "ok"`.
    #[must_use]
    pub fn ok(operation: &'static str, data: T) -> Self {
        Self {
            status: "ok",
            operation,
            data,
        }
    }

    /// Dry-run preview envelope: `status = "dry_run"`.
    #[must_use]
    pub fn dry_run(operation: &'static str, data: T) -> Self {
        Self {
            status: "dry_run",
            operation,
            data,
        }
    }
}

impl Envelope<ErrorData> {
    /// Failure envelope: `status = "error"` with a flattened [`ErrorData`].
    #[must_use]
    pub fn error(operation: &'static str, data: ErrorData) -> Self {
        Self {
            status: "error",
            operation,
            data,
        }
    }
}

/// Error payload: stable `code`, human `message`, actionable `hint`.
#[derive(Debug, Serialize)]
pub struct ErrorData {
    /// Machine-readable code, e.g. `TARGET_EXISTS`, `SOURCE_NOT_FOUND`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// What the caller should do next (omitted from JSON when absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl ErrorData {
    /// Build an error payload from a stable code, message and optional hint.
    #[must_use]
    pub fn new(code: &str, message: String, hint: Option<String>) -> Self {
        Self {
            code: code.to_owned(),
            message,
            hint,
        }
    }

    /// Map an engine error onto a stable code plus an actionable hint.
    #[must_use]
    pub fn from_core(err: &JmoveError) -> Self {
        let (code, hint) = match err {
            JmoveError::Io(_) => ("IO_ERROR", "check file permissions and disk space"),
            JmoveError::InvalidArgument(_) => (
                "INVALID_ARGUMENT",
                "paths must be inside the project root given by --root",
            ),
            JmoveError::StaleIndex(_) => (
                "STALE_INDEX",
                "rerun the command; the index is rebuilt on every run",
            ),
            JmoveError::PlanRejected(_) => (
                "PLAN_REJECTED",
                "run `jmove check --json` to inspect the import graph",
            ),
            JmoveError::Git(_) => (
                "GIT_ERROR",
                "retry with --no-git to move without touching git",
            ),
        };
        Self::new(code, err.to_string(), Some(hint.to_owned()))
    }
}

/// One rewritten import inside a changed file.
#[derive(Debug, Serialize)]
pub struct Change {
    /// 1-based line of the rewritten specifier.
    pub line: usize,
    /// Specifier text before the move.
    pub old: String,
    /// Specifier text after the move.
    pub new: String,
}

/// A file whose imports were rewritten, with line-level change details.
#[derive(Debug, Serialize)]
pub struct ChangedFile {
    /// Project-relative path of the importer.
    pub path: String,
    /// Rewritten specifiers, in source order.
    pub changes: Vec<Change>,
}

/// Success payload of `mv --json` (flattened under `status: "ok"`).
#[derive(Debug, Serialize)]
pub struct MvData {
    /// Project-relative path the file moved from.
    pub source: String,
    /// Project-relative path the file moved to.
    pub target: String,
    /// Importer files touched by the move.
    pub changed_files: Vec<ChangedFile>,
    /// Number of files moved (always 1 in Phase 1).
    pub moved: usize,
    /// Total specifiers rewritten across all importers.
    pub updated_imports: usize,
    /// Rename backend: `"git"` (staged in the index) or `"fs"`.
    pub moved_via: &'static str,
}

impl MvData {
    /// Assemble the payload from an applied plan and its change details.
    #[must_use]
    pub fn new(plan: &MovePlan, changed_files: Vec<ChangedFile>, via_git: bool) -> Self {
        Self {
            source: rel_str(&plan.source),
            target: rel_str(&plan.target),
            changed_files,
            moved: 1,
            updated_imports: plan.rewrites.len(),
            moved_via: if via_git { "git" } else { "fs" },
        }
    }
}

/// Dry-run payload of `mv --dry-run --json` (flattened under
/// `status: "dry_run"`).
#[derive(Debug, Serialize)]
pub struct MvDryRunData {
    /// Project-relative path that would move.
    pub would_move: String,
    /// Project-relative destination that would be created.
    pub target: String,
    /// Specifiers that would be rewritten.
    pub would_update: usize,
    /// Importer files that would be touched, sorted.
    pub affected_files: Vec<String>,
    /// Unified diff (rewrites + rename) of the whole plan.
    pub diff: String,
    /// Rename backend a real run would use: `"git"` or `"fs"`.
    pub would_move_via: &'static str,
}

impl MvDryRunData {
    /// Assemble the preview payload from a plan and its rendered diff.
    #[must_use]
    pub fn new(plan: &MovePlan, diff: String, via_git: bool) -> Self {
        Self {
            would_move: rel_str(&plan.source),
            target: rel_str(&plan.target),
            would_update: plan.rewrites.len(),
            affected_files: output::group_by_file(&plan.rewrites)
                .into_iter()
                .map(|(file, _)| rel_str(file))
                .collect(),
            diff,
            would_move_via: if via_git { "git" } else { "fs" },
        }
    }
}

/// One unresolvable relative import found by `check`.
#[derive(Debug, Serialize)]
pub struct BrokenImport {
    /// Project-relative file declaring the import.
    pub file: String,
    /// 1-based line of the specifier.
    pub line: usize,
    /// Specifier text as written.
    pub import: String,
    /// Stable reason code, currently always `"file_not_found"`.
    pub reason: &'static str,
}

/// Success payload of `check --json` (flattened under the envelope).
#[derive(Debug, Serialize)]
pub struct CheckData {
    /// Broken imports, sorted by file then line.
    pub broken_imports: Vec<BrokenImport>,
    /// Number of broken imports (kept as an explicit counter for agents).
    pub total: usize,
}

/// Serialize `value` as pretty JSON to stdout.
pub fn print<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(err) => eprintln!("jmove: failed to serialize JSON output: {err}"),
    }
}
