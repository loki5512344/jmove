//! Human-readable rendering plus the payload builders that both output
//! modes share: grouping rewrites, resolving broken imports, line lookup.
//!
//! Pure functions returning data, except [`report_check`] and
//! [`print_error`] which perform the only I/O (stdout and stderr).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::core::JmoveResult;
use crate::core::index::Index;
use crate::core::plan::{MovePlan, Rewrite};

use super::json::{BrokenImport, Change, ChangedFile};

/// `check` stdout line when the project has no broken imports.
const CHECK_CLEAN: &str = "check: no broken imports found";

/// Read a project file (project-relative path) as UTF-8 text.
fn read_file(root: &Path, rel: &Path) -> JmoveResult<String> {
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

/// Collect every relative import that resolves to nothing in `index`.
///
/// A specifier starting with `.` whose target is `None` is broken; a bare
/// package specifier without a target is an external dependency, not an
/// error. Results are sorted by file, then line.
pub fn broken_imports(root: &Path, index: &Index) -> JmoveResult<Vec<BrokenImport>> {
    let mut broken: Vec<BrokenImport> = Vec::new();
    for (file, imports) in &index.imports {
        for import in imports {
            if import.target.is_some() || !import.record.specifier.starts_with('.') {
                continue;
            }
            let text = read_file(root, file)?;
            broken.push(BrokenImport {
                file: file.display().to_string(),
                line: line_of(&text, import.record.span.start),
                import: import.record.specifier.clone(),
                reason: "file_not_found",
            });
        }
    }
    broken.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(broken)
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
            path: path.display().to_string(),
            changes,
        })
        .collect())
}

/// `moved src -> tgt, updated N imports in M files` success summary.
#[must_use]
pub fn mv_summary(plan: &MovePlan) -> String {
    let imports = plan.rewrites.len();
    let files = group_by_file(&plan.rewrites).len();
    format!(
        "moved {} -> {}, updated {} {} in {} {}",
        plan.source.display(),
        plan.target.display(),
        imports,
        plural(imports, "import"),
        files,
        plural(files, "file"),
    )
}

/// Print the human `check` report: the clean note, or one
/// `path:line: cannot resolve 'spec'` line per broken import.
pub fn report_check(broken: &[BrokenImport]) {
    if broken.is_empty() {
        println!("{CHECK_CLEAN}");
        return;
    }
    for entry in broken {
        let line = format!(
            "{}:{}: cannot resolve '{}'",
            entry.file, entry.line, entry.import
        );
        println!("{line}");
    }
}

/// Print an operation error to stderr, with the hint on its own line.
pub fn print_error(message: &str, hint: Option<&str>) {
    eprintln!("jmove: {message}");
    if let Some(hint) = hint {
        eprintln!("  hint: {hint}");
    }
}

/// `N noun` with a naive English plural.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}
