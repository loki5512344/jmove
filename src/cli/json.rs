//! JSON output shapes shared by all `--json` commands.
//!
//! Every response is an [`Envelope`] whose `status` is one of
//! `"ok" | "dry_run" | "error"`, plus a machine-readable error `code`
//! and a human `hint` on failure (see `docs/SKILL.md`). Payload structs
//! are flattened into the envelope, so they never repeat `operation`.

use serde::Serialize;

use crate::core::plan::MovePlan;
use crate::core::refs::NonImportRef;
use crate::core::{JmoveError, rel_str};

use super::output::ChangedFile;
use super::output::{self};

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

/// Success payload of `mv --json` (flattened under `status: "ok"`).
#[derive(Debug, Serialize)]
pub struct MvData {
    /// Project-relative path the file moved from.
    pub source: String,
    /// Project-relative path the file moved to.
    pub target: String,
    /// Importer files touched by the move.
    pub changed_files: Vec<ChangedFile>,
    /// Number of files physically moved (1 for a file move, N for a dir).
    pub moved: usize,
    /// Total specifiers rewritten across all importers.
    pub updated_imports: usize,
    /// Rename backend: `"git"` (every rename staged in the index) or `"fs"`.
    pub moved_via: &'static str,
    /// Non-import textual references left unfixed (omitted when none).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub non_import_refs: Vec<NonImportRef>,
    /// Directory moves only: each `(from, to)` relocation (omitted for file moves).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub moved_files: Vec<FileMoveData>,
    /// Directory moves only: unindexable files staying in place (omitted when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub left_behind: Vec<String>,
}

/// One `(from, to)` relocation of a directory move.
#[derive(Debug, Serialize)]
pub struct FileMoveData {
    /// Project-relative path moved away.
    pub from: String,
    /// Project-relative destination path.
    pub to: String,
}

impl FileMoveData {
    fn from(m: &crate::core::plan::FileMove) -> Self {
        Self {
            from: rel_str(&m.source),
            to: rel_str(&m.target),
        }
    }
}

// Relocation list, present only when the plan moved more than one file.
fn dir_moves(plan: &MovePlan) -> Vec<FileMoveData> {
    if plan.moves.len() > 1 {
        plan.moves.iter().map(FileMoveData::from).collect()
    } else {
        Vec::new()
    }
}

impl MvData {
    /// Assemble the payload from an applied plan and its change details.
    #[must_use]
    pub fn new(
        plan: &MovePlan,
        changed_files: Vec<ChangedFile>,
        via_git: bool,
        non_import_refs: Vec<NonImportRef>,
    ) -> Self {
        Self {
            source: rel_str(&plan.source),
            target: rel_str(&plan.target),
            changed_files,
            moved: plan.moves.len(),
            updated_imports: plan.rewrites.len(),
            moved_via: if via_git { "git" } else { "fs" },
            non_import_refs,
            // A single-file move keeps the old contract: no extra fields.
            moved_files: dir_moves(plan),
            left_behind: plan.left_behind.iter().map(|p| rel_str(p)).collect(),
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
    /// Non-import references a real run would leave unfixed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub non_import_refs: Vec<NonImportRef>,
    /// Directory moves only: every relocation that would happen.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub would_move_files: Vec<FileMoveData>,
}

impl MvDryRunData {
    /// Assemble the preview payload from a plan and its rendered diff.
    #[must_use]
    pub fn new(
        plan: &MovePlan,
        diff: String,
        via_git: bool,
        non_import_refs: Vec<NonImportRef>,
    ) -> Self {
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
            would_move_files: dir_moves(plan),
            non_import_refs,
        }
    }
}

/// Serialize `value` as pretty JSON to stdout.
pub fn print<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(err) => eprintln!("jmove: failed to serialize JSON output: {err}"),
    }
}
