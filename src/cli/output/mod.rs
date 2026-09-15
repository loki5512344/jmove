//! Human-readable rendering plus the payload builders that both output
//! modes share: grouping rewrites, `mv` pre-flight validation, line lookup.
//! The `check` payloads and collectors live in [`check`].
//!
//! Pure functions returning data, except [`report_check`] and
//! [`print_error`] which perform the only I/O (stdout and stderr).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::core::plan::{MovePlan, Rewrite};
use crate::core::{JmoveResult, rel_str};

mod check;

use super::json::{Change, ChangedFile, ErrorData};

pub use check::{
    BrokenImport, CheckData, NameMismatch, broken_imports, name_mismatches, report_check,
};

/// Read a project file (project-relative path) as UTF-8 text.
pub(crate) fn read_file(root: &Path, rel: &Path) -> JmoveResult<String> {
    Ok(std::fs::read_to_string(root.join(rel))?)
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

/// Group rewrites by importer file; files in sorted order, rewrites of one
/// file keep their source order. Shared by JSON payloads and summaries.
#[must_use]
pub fn group_by_file(rewrites: &[Rewrite]) -> Vec<(&Path, Vec<&Rewrite>)> {
    let mut map: BTreeMap<&Path, Vec<&Rewrite>> = BTreeMap::new();
    for rewrite in rewrites {
        map.entry(&rewrite.file).or_default().push(rewrite);
    }
    map.into_iter().collect()
}
/// Build the `changed_files` payload: line-level specifier diffs grouped per
/// importer, computed against the on-disk contents at call time.
pub fn changed_files(root: &Path, plan: &MovePlan) -> JmoveResult<Vec<ChangedFile>> {
    let mut per_file: BTreeMap<&PathBuf, Vec<Change>> = BTreeMap::new();
    let mut contents: BTreeMap<&PathBuf, String> = BTreeMap::new();
    for rewrite in &plan.rewrites {
        if !contents.contains_key(&rewrite.file) {
            contents.insert(&rewrite.file, read_file(root, &rewrite.file)?);
        }
        // The key was just ensured, so this lookup cannot fail.
        let line = line_of(&contents[&rewrite.file], rewrite.span.start);
        let change = Change {
            line,
            old: rewrite.old_text.clone(),
            new: rewrite.new_text.clone(),
        };
        per_file.entry(&rewrite.file).or_default().push(change);
    }
    Ok(per_file
        .into_iter()
        .map(|(path, changes)| ChangedFile {
            path: rel_str(path),
            changes,
        })
        .collect())
}

/// Pre-flight `mv` validation: `(root, source, target)` are project-relative
/// and normalized. A file that exists on disk but is absent from the import
/// index stays moveable: its plan simply has no rewrites.
#[must_use]
pub fn mv_reject(root: &Path, source: &Path, target: &Path) -> Option<ErrorData> {
    let bad = |code: &str, message: String, hint: &str| {
        Some(ErrorData::new(code, message, Some(hint.into())))
    };
    if source == target {
        let msg = "source and target are the same path".into();
        return bad("INVALID_ARGUMENT", msg, "pick a different destination");
    }
    let src_path = root.join(source);
    if !src_path.is_file() && !src_path.is_dir() {
        let msg = format!("source file '{}' does not exist", rel_str(source));
        return bad(
            "SOURCE_NOT_FOUND",
            msg,
            "check the path or run `jmove check`",
        );
    }
    if root.join(target).exists() {
        let msg = format!("target path '{}' already exists", rel_str(target));
        return bad(
            "TARGET_EXISTS",
            msg,
            "remove or rename the existing target first",
        );
    }
    // `target` names a file, so `parent()` always yields the directory part.
    let parent = root.join(target.parent().unwrap_or(Path::new("")));
    if parent.exists() && !parent.is_dir() {
        let msg = format!("target parent of '{}' is not a directory", rel_str(target));
        return bad(
            "INVALID_ARGUMENT",
            msg,
            "pick a destination inside a directory",
        );
    }
    None
}

/// `moved src -> tgt, updated N imports in M files` success summary,
/// noting the `git mv` backend and, for directory moves, the file count
/// and anything unindexable that stays behind.
#[must_use]
pub fn mv_summary(plan: &MovePlan, via_git: bool) -> String {
    let imports = plan.rewrites.len();
    let files = group_by_file(&plan.rewrites).len();
    let git = if via_git { " (via git mv)" } else { "" };
    let batch = match plan.moves.len() {
        1 => String::new(),
        n => format!(" ({n} files)"),
    };
    let left = match plan.left_behind.len() {
        0 => String::new(),
        n => format!(", {} unsupported {} left behind", n, plural(n, "file")),
    };
    format!(
        "moved {} -> {}{batch}{git}, updated {} {} in {} {}{left}",
        rel_str(&plan.source),
        rel_str(&plan.target),
        imports,
        plural(imports, "import"),
        files,
        plural(files, "file"),
    )
}
/// Print an operation error to stderr, with the hint on its own line.
pub fn print_error(message: &str, hint: Option<&str>) {
    eprintln!("jmove: {message}");
    if let Some(hint) = hint {
        eprintln!("  hint: {hint}");
    }
}

/// `N noun` with a naive English plural.
pub(crate) fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}
