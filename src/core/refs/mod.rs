//! Non-import textual references to planned moves: report only.
//!
//! The import graph understands `import`/`export from`/`require` and
//! `import()` — but projects also reference files from markdown links,
//! `package.json` fields, `jest.mock("./x")` strings, tsconfig `files`
//! arrays and friends. Auto-rewriting those would require understanding
//! each format's semantics, so `jmove` never edits them; what it must not
//! do is move a file and stay silent while half a readme keeps pointing at
//! the old path. The scan runs before any write, on the same plan the
//! apply step consumes, and returns every suspicious occurrence with its
//! file and line.
//!
//! Matching is boundary-checked substring search over plain tokens (the
//! moved path, its extension-less module form, the exact specifier strings
//! importers used, and the directory prefix for batch moves). Tokens are
//! deliberately conservative: `lib/sum` matches `./lib/sum.ts` but not
//! `lib/summary`; occurrences inside the plan's own rewrite spans are the
//! import statements and are excluded.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::index::Index;
use crate::core::plan::MovePlan;
use crate::core::{line_of, normalize_rel_path, rel_str};

/// One non-import occurrence of a moved path or specifier.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NonImportRef {
    /// Project-relative file containing the reference.
    pub file: String,
    /// 1-based line of the occurrence.
    pub line: usize,
    /// The matched token.
    pub token: String,
    /// Category of the token: `path`, `module`, `specifier` or `dir`.
    /// Patterns are ranked in this order; one entry is kept per line.
    pub kind: &'static str,
    /// The matched line, trimmed and capped (context for humans/agents).
    pub text: String,
}

/// Text formats beyond source code that may name project files.
const TEXT_EXTS: &[&str] = &[
    "md", "mdx", "json", "json5", "yaml", "yml", "html", "htm", "xml", "txt", "pro", "gradle",
];

/// Lockfiles never reference project-local paths and are huge: skipped.
const LOCKFILES: &[&str] = &["package-lock.json", "yarn.lock", "pnpm-lock.yaml"];

const MAX_TEXT_BYTES: u64 = 512 * 1024;

/// Scan the project for references to `plan`'s moves outside the import
/// statements the plan already rewrites. Infallible by design: unreadable
/// files are skipped (the scanner already ignored what it could not read).
pub fn scan(root: &Path, index: &Index, plan: &MovePlan) -> Vec<NonImportRef> {
    let patterns = patterns(root, plan);
    if patterns.is_empty() {
        return Vec::new();
    }
    let mut refs = Vec::new();
    for file in text_files(root, index) {
        let Ok(source) = std::fs::read_to_string(root.join(&file)) else {
            continue; // binary or vanished between walk and read
        };
        let skip = skip_spans(plan, &file);
        for (token, kind) in &patterns {
            for at in token_occurrences(&source, token) {
                if skip.iter().any(|s| s.contains(&at)) {
                    continue; // the import statements themselves
                }
                refs.push(NonImportRef {
                    file: rel_str(&file),
                    line: line_of(&source, at),
                    token: token.clone(),
                    kind,
                    text: excerpt(&source, at),
                });
            }
        }
    }
    // Stable sort keeps the first-inserted (highest-ranked) token per line:
    // one warning per reference site, not one per matching pattern.
    refs.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    refs.dedup_by(|a, b| a.file == b.file && a.line == b.line);
    refs
}

// The search tokens of a plan. `source_root` scoping needs no special
// treatment: every token is already project-relative.
fn patterns(root: &Path, plan: &MovePlan) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut push = |token: String, kind: &'static str| {
        if token.len() > 1 && !out.iter().any(|(t, _)| t == &token) {
            out.push((token, kind));
        }
    };
    for m in &plan.moves {
        let path = rel_str(&m.source);
        push(path.clone(), "path");
        if let Some((stem, _)) = path.rsplit_once('.') {
            push(stem.to_owned(), "module");
        }
    }
    for rewrite in &plan.rewrites {
        push(rewrite.old_text.clone(), "specifier");
    }
    if plan.moves.len() > 1 && root.join(&plan.source).is_dir() {
        push(format!("{}/", rel_str(&plan.source)), "dir");
    }
    out
}

// Files whose rewrite the plan already performs legitimately: their spans.
fn skip_spans(plan: &MovePlan, file: &Path) -> Vec<std::ops::Range<usize>> {
    plan.rewrites
        .iter()
        .filter(|r| r.file == file)
        .map(|r| r.span.clone())
        .collect()
}

/// Code files from the index plus non-hidden text files on disk.
fn text_files(root: &Path, index: &Index) -> Vec<PathBuf> {
    let mut out = index.files.sorted();
    let walker = ignore::WalkBuilder::new(root).require_git(false).build();
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.path_is_symlink() || !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if LOCKFILES.contains(&name) {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !TEXT_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let Ok(stripped) = path.strip_prefix(root) else {
            continue;
        };
        let Some(rel) = normalize_rel_path(stripped) else {
            continue;
        };
        let hidden = rel
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
        let small = path
            .metadata()
            .map(|m| m.len() <= MAX_TEXT_BYTES)
            .unwrap_or(false);
        if !hidden && small && !out.contains(&rel) {
            out.push(rel);
        }
    }
    out.sort();
    out
}

/// Every offset where `token` occurs with a word-free byte before it and
/// no word/joiner byte after: `lib/sum` matches `./lib/sum.ts` but never
/// `lib/summary` or `lib/sum-2`.
fn token_occurrences(source: &str, token: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(found) = source[from..].find(token) {
        let at = from + found;
        let end = at + token.len();
        let before_ok = at == 0 || !is_word_byte(bytes[at - 1]);
        let after_ok = !bytes
            .get(end)
            .is_some_and(|c| is_word_byte(*c) || matches!(c, b'-' | b'_'));
        if before_ok && after_ok {
            hits.push(at);
        }
        from = end;
    }
    hits
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// The matched line, trimmed, with a hard cap.
fn excerpt(source: &str, at: usize) -> String {
    let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[at..].find('\n').map_or(source.len(), |i| at + i);
    let line = source[line_start..line_end].trim();
    let cut = line.char_indices().nth(100).map_or(line.len(), |(i, _)| i);
    line[..cut].to_owned()
}

#[cfg(test)]
mod tests;
