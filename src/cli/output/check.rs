//! `jmove check` collectors and payloads: unresolvable relative imports
//! plus Java file-name ⇄ public-class mismatches (layout errors that
//! `javac` rejects but import resolution cannot see).

use serde::Serialize;
use std::path::Path;

use crate::core::index::Index;
use crate::core::{JmoveResult, rel_str};
use crate::parser::SourceLanguage;
use crate::parser::java::class_name;

use super::{line_of, read_file};

/// `check` stdout line when the project has no findings.
const CHECK_CLEAN: &str = "check: no findings";

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

/// A Java file whose single public top-level type is named differently
/// from the file — a `javac` error repaired by renaming the file (imports
/// stay valid: the class FQN does not change).
#[derive(Debug, Serialize)]
pub struct NameMismatch {
    /// Project-relative file with the wrong name.
    pub file: String,
    /// 1-based line of the public type declaration.
    pub line: usize,
    /// Declared public type.
    pub public_class: String,
    /// Project-relative file it should live in.
    pub expected_file: String,
    /// Copy-paste repair command (paths are quoted for safety).
    pub rename: String,
}

/// Success payload of `check --json` (flattened under the envelope).
#[derive(Debug, Serialize)]
pub struct CheckData {
    /// Broken imports, sorted by file then line.
    pub broken_imports: Vec<BrokenImport>,
    /// Number of broken imports (kept as an explicit counter for agents).
    pub total: usize,
    /// Java layout findings (omitted from JSON when clean).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub name_mismatches: Vec<NameMismatch>,
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
                file: rel_str(file),
                line: line_of(&text, import.record.span.start),
                import: import.record.specifier.clone(),
                reason: "file_not_found",
            });
        }
    }
    broken.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(broken)
}

/// Java files whose only public top-level type disagrees with the file
/// name; sorted by file, then line.
pub fn name_mismatches(root: &Path, index: &Index) -> JmoveResult<Vec<NameMismatch>> {
    let mut out = Vec::new();
    for file in index.files.sorted() {
        if SourceLanguage::for_path(&file) != Some(SourceLanguage::Java) {
            continue;
        }
        let text = read_file(root, &file)?;
        let Some(found) = class_name::mismatch(&file, &text) else {
            continue;
        };
        let dir = file.parent().unwrap_or(Path::new(""));
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("java");
        let expected = dir.join(format!("{}.{}", found.public_class, ext));
        let (old, new) = (rel_str(&file), rel_str(&expected));
        out.push(NameMismatch {
            file: old.clone(),
            line: line_of(&text, found.span.start),
            public_class: found.public_class,
            expected_file: new.clone(),
            rename: format!("jmove mv '{old}' '{new}'"),
        });
    }
    out.sort_by_key(|m| (m.file.clone(), m.line));
    Ok(out)
}

/// Print the human `check` report: the clean note, or one
/// `path:line: cannot resolve 'spec'` line per broken import and one
/// rename-line per Java layout mismatch.
pub fn report_check(broken: &[BrokenImport], mismatches: &[NameMismatch]) {
    if broken.is_empty() && mismatches.is_empty() {
        println!("{CHECK_CLEAN}");
        return;
    }
    for entry in broken {
        println!(
            "{}:{}: cannot resolve '{}'",
            entry.file, entry.line, entry.import
        );
    }
    for m in mismatches {
        println!(
            "{}:{}: public class '{}' must live in '{}'; fix: {}",
            m.file, m.line, m.public_class, m.expected_file, m.rename
        );
    }
}
