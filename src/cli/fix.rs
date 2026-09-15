//! `jmove fix` — auto-repair small import problems with the same safety
//! engine as `mv`: index → rules → plan → optional dry-run diff → atomic
//! apply. Deterministic by design; ambiguous findings are reported as
//! non-auto `candidates` for an agent to resolve, never applied blindly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::apply::{self, render_edits_diff};
use crate::core::fix::{FixPlan, plan_fix};
use crate::core::index::Index;
use crate::core::{JmoveError, JmoveResult, rel_str};
use crate::parser;

use super::json::Envelope;
use super::{Flow, exit, fail, flow, json, output};

/// One reported candidate (JSON element). `applied` is false for
/// ambiguous findings the engine will not touch.
#[derive(Debug, Serialize)]
pub struct FixEntry {
    /// Rule that produced this candidate, e.g. `java/unused-import`.
    pub rule: &'static str,
    /// 1-based line of the first edit.
    pub line: usize,
    /// `error` | `warning` | `info`.
    pub severity: &'static str,
    /// `true` = auto-applied; `false` = needs an agent/human decision.
    pub applied: bool,
    /// Human explanation, mirrors `FixCandidate::message`.
    pub message: String,
    /// FQN options for an ambiguous finding (omitted when empty): pick one,
    /// add the import yourself and re-run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
}

/// A file touched (or inspectable) by `fix`.
#[derive(Debug, Serialize)]
pub struct FixedFile {
    /// Project-relative path.
    pub path: String,
    /// Candidates for this file, in source order.
    pub fixes: Vec<FixEntry>,
}

/// Apply-mode payload (`status: "ok"`).
#[derive(Debug, Serialize)]
pub struct FixData {
    /// Candidates applied to disk.
    pub fixes: usize,
    /// Files rewritten.
    pub files_changed: usize,
    /// Per-file detail, sorted by path.
    pub changed_files: Vec<FixedFile>,
}

/// Dry-run payload (`status: "dry_run"`).
#[derive(Debug, Serialize)]
pub struct FixDryRunData {
    /// Candidates that would be applied.
    pub would_fix: usize,
    /// Files that would be rewritten, sorted.
    pub affected_files: Vec<String>,
    /// Unified diff of the whole plan.
    pub diff: String,
    /// Per-file candidate detail, sorted by path.
    pub files: Vec<FixedFile>,
}

/// `fix` handler: validate rule, index, plan, then dry-run or apply.
pub fn fix(
    root: &Path,
    source_root: Option<&Path>,
    rule: Option<&str>,
    dry_run: bool,
    json: bool,
) -> Flow<i32> {
    let root = flow(json, "fix", root.canonicalize().map_err(JmoveError::from))?;
    let source_root = flow(json, "fix", Index::normalize_scope(&root, source_root))?;
    if let Some(rejected) = fix_reject(rule) {
        return Err(fail(json, "fix", rejected));
    }
    let scope = source_root.as_deref();
    let index = flow(json, "fix", Index::build_scoped(&root, scope))?;
    let plan = plan_fix(&index, rule);
    if plan.is_empty() {
        if json {
            json::print(&Envelope::ok("fix", empty_data()));
        } else {
            println!("fix: nothing to change");
        }
        return Ok(exit::OK);
    }
    if dry_run {
        return fix_dry_run(&root, json, &plan);
    }
    // Line numbers use spans against the original contents, so the JSON
    // payload is assembled before any edit reaches the disk.
    let detail = flow(json, "fix", describe(&root, &plan))?;
    let edits = plan.auto_edits();
    let files_changed = flow(json, "fix", apply::apply_edits(&root, &edits))?;
    if json {
        let fixes = edits.values().map(Vec::len).sum();
        json::print(&Envelope::ok(
            "fix",
            FixData {
                fixes,
                files_changed,
                changed_files: detail,
            },
        ));
    } else {
        let files = edits.len();
        let fixes = edits.values().map(Vec::len).sum();
        if files == 0 {
            // Manual-only plan (e.g. ambiguous `java/missing-import`):
            // nothing applied, the agent decides via --json candidates.
            let manual = plan.total();
            println!(
                "fix: nothing to change; {manual} manual {} (see --json)",
                output::plural(manual, "candidate")
            );
        } else {
            println!(
                "fixed {} {} in {} {}",
                fixes,
                output::plural(fixes, "issue"),
                files,
                output::plural(files, "file")
            );
        }
    }
    Ok(exit::OK)
}

/// Reject an unknown `--rule` before doing any work.
fn fix_reject(rule: Option<&str>) -> Option<super::json::ErrorData> {
    let name = rule?;
    if parser::rule_ids().contains(&name) {
        return None;
    }
    let known = parser::rule_ids().join(", ");
    Some(super::json::ErrorData::new(
        "INVALID_ARGUMENT",
        format!("unknown fix rule '{name}'"),
        Some(format!("available rules: {known}")),
    ))
}

/// Dry-run branch: unified diff for humans, structured preview for agents.
fn fix_dry_run(root: &Path, json: bool, plan: &FixPlan) -> Flow<i32> {
    let edits = plan.auto_edits();
    let diff = flow(json, "fix", render_edits_diff(root, &edits))?;
    if json {
        let detail = flow(json, "fix", describe(root, plan))?;
        let affected = edits.keys().map(|p| rel_str(p)).collect();
        json::print(&Envelope::dry_run(
            "fix",
            FixDryRunData {
                would_fix: edits.values().map(Vec::len).sum(),
                affected_files: affected,
                diff,
                files: detail,
            },
        ));
    } else {
        print!("{diff}");
    }
    Ok(exit::OK)
}

/// Empty-plan payload with zero counters.
fn empty_data() -> FixData {
    FixData {
        fixes: 0,
        files_changed: 0,
        changed_files: Vec::new(),
    }
}

// Build per-file candidate detail with 1-based line numbers resolved
// against the on-disk contents at call time (before any apply).
fn describe(root: &Path, plan: &FixPlan) -> JmoveResult<Vec<FixedFile>> {
    let mut lines: BTreeMap<&PathBuf, String> = BTreeMap::new();
    let mut out = Vec::new();
    for (file, candidates) in &plan.files {
        if !lines.contains_key(file) {
            lines.insert(file, std::fs::read_to_string(root.join(file))?);
        }
        let source = &lines[file];
        let fixes = candidates
            .iter()
            .map(|c| FixEntry {
                rule: c.rule,
                line: output::line_of(source, c.span.start),
                severity: c.severity.as_str(),
                applied: c.auto_fixable,
                message: c.message.clone(),
                candidates: c.candidates.clone(),
            })
            .collect();
        out.push(FixedFile {
            path: rel_str(file),
            fixes,
        });
    }
    Ok(out)
}
