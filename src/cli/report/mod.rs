//! Machine-readable report files, and the `check` command that produces
//! their primary findings.
//!
//! `--report <FILE>` (on `check` and `fix`) writes the same findings a run
//! already computes into a CI-consumable format chosen by file extension:
//! SARIF 2.1.0 (`.sarif` — GitHub code scanning, CodeQL upload) or
//! Checkstyle XML (`.xml` — IDEs, Jenkins, GitLab). Unknown extensions
//! fail early with `INVALID_ARGUMENT`, before any indexing.
//!
//! Reports never change stdout or exit codes: `check` still exits `2` when
//! it finds something, and a clean run still writes a *valid empty* report
//! (CI parsers must not choke on green builds).

use std::path::{Path, PathBuf};

use crate::core::index::Index;
use crate::core::{JmoveError, JmoveResult};

use crate::cli::fix::FixedFile;
use crate::cli::json::Envelope;
use crate::cli::output::{BrokenImport, NameMismatch};
use crate::cli::{Flow, exit, flow, json, output};

mod checkstyle;
mod sarif;

/// One finding, detached from the format that renders it.
#[derive(Debug)]
pub struct Violation {
    /// Stable rule id, e.g. `java/unused-import` or `broken-import`.
    pub rule: &'static str,
    /// `error` | `warning` | `info` (mirrors `--json`).
    pub severity: &'static str,
    /// Human-readable message, same text the terminal output shows.
    pub message: String,
    /// Project-relative file path, `/` separated.
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// Whether jmove can resolve it itself (fix engine, or the suggested
    /// `jmove mv` for layout findings).
    pub fixable: bool,
}

/// A parsed `--report` destination.
#[derive(Debug)]
pub enum Report {
    /// SARIF 2.1.0 document.
    Sarif(PathBuf),
    /// Checkstyle XML document.
    Checkstyle(PathBuf),
}

impl Report {
    /// Validate a requested report path (by extension) without touching it.
    pub fn parse(requested: Option<&Path>) -> JmoveResult<Option<Self>> {
        let Some(path) = requested else {
            return Ok(None);
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let report = if name.ends_with(".sarif") {
            Self::Sarif(path.to_path_buf())
        } else if name.ends_with(".xml") {
            Self::Checkstyle(path.to_path_buf())
        } else {
            return Err(JmoveError::InvalidArgument(format!(
                "--report '{name}': unsupported file name"
            )));
        };
        Ok(Some(report))
    }

    /// Write the report when one was requested; always valid, also empty.
    pub fn emit(sink: &Option<Self>, violations: &[Violation]) -> JmoveResult<()> {
        let Some(report) = sink else {
            return Ok(());
        };
        let (path, content) = match report {
            Self::Sarif(path) => (path, sarif::build(violations)),
            Self::Checkstyle(path) => (path, checkstyle::build(violations)),
        };
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Findings of `check` as report violations.
#[must_use]
pub fn from_check(broken: &[BrokenImport], mismatches: &[NameMismatch]) -> Vec<Violation> {
    let mut out: Vec<Violation> = broken
        .iter()
        .map(|b| Violation {
            rule: "broken-import",
            severity: "error",
            message: format!("cannot resolve '{}'", b.import),
            file: b.file.clone(),
            line: b.line,
            fixable: false,
        })
        .collect();
    out.extend(mismatches.iter().map(|m| Violation {
        rule: "java/class-name-mismatch",
        severity: "error",
        message: format!(
            "public class '{}' must live in '{}'; {}",
            m.public_class, m.expected_file, m.rename
        ),
        file: m.file.clone(),
        line: m.line,
        fixable: true,
    }));
    out
}

/// Findings of `fix` (the per-file candidate detail) as report violations.
#[must_use]
pub fn from_fix(files: &[FixedFile]) -> Vec<Violation> {
    files
        .iter()
        .flat_map(|f| {
            f.fixes.iter().map(|c| Violation {
                rule: c.rule,
                severity: c.severity,
                message: c.message.clone(),
                file: f.path.clone(),
                line: c.line,
                fixable: c.applied,
            })
        })
        .collect()
}

/// `check` handler: report relative imports that resolve to nothing.
///
/// Exit code is `2` when at least one finding exists, in both the human
/// and the `--json` mode (the JSON `status` stays `"ok"` — the command
/// itself succeeded; agents read `total` or the exit code). A `--report`
/// file is written regardless of mode and does not alter the exit code.
pub fn check(
    root: &Path,
    source_root: Option<&Path>,
    json: bool,
    report: Option<&Path>,
) -> Flow<i32> {
    let sink = flow(json, "check", Report::parse(report))?;
    let root = flow(json, "check", root.canonicalize().map_err(JmoveError::from))?;
    let source_root = flow(json, "check", Index::normalize_scope(&root, source_root))?;
    let scope = source_root.as_deref();
    let index = flow(json, "check", Index::build_scoped(&root, scope))?;
    let broken = flow(json, "check", output::broken_imports(&root, &index))?;
    let mismatches = flow(json, "check", output::name_mismatches(&root, &index))?;
    flow(
        json,
        "check",
        Report::emit(&sink, &from_check(&broken, &mismatches)),
    )?;
    let code = if broken.is_empty() && mismatches.is_empty() {
        exit::OK
    } else {
        exit::BROKEN
    };

    if json {
        let total = broken.len();
        let data = output::CheckData {
            broken_imports: broken,
            total,
            name_mismatches: mismatches,
        };
        json::print(&Envelope::ok("check", data));
    } else {
        output::report_check(&broken, &mismatches);
    }
    Ok(code)
}
