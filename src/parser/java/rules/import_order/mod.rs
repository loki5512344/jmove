//! Java rule: normalise the import block to Google order — static imports
//! first, then single-type imports, each group ASCII-sorted, duplicates
//! dropped, one blank line between the groups.
//!
//! The repair is a single replace-span over the whole (contiguous) import
//! block, so `old_text` doubles as the staleness guard: if anything moved
//! between index and apply, the engine rejects before writing.
//!
//! Bail-out rules keep the direction of the other Java rules — never
//! rewrite something the rule cannot fully account for: fewer than two
//! imports is never dirty; a block containing comments or any non-import
//! statement is left alone (attached comments must not be orphaned by a
//! re-sort); an already-normalised block produces no candidate at all.

use std::path::Path;

use tree_sitter::Node;

use super::is_static_line;
use crate::core::Edit;
use crate::core::index::Index;
use crate::parser::java::{TreeSitterJava, line_end, text};
use crate::parser::{Fix, FixCandidate, Severity};

/// Rule id accepted by `jmove fix --rule`.
pub const RULE: &str = "java/import-order";

/// Import block orderer.
pub struct JavaImportOrder;

impl JavaImportOrder {
    /// Create the rule.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for JavaImportOrder {
    fn default() -> Self {
        Self::new()
    }
}

impl Fix for JavaImportOrder {
    fn rule(&self) -> &'static str {
        RULE
    }

    fn fixes(&self, _path: &Path, source: &str, _index: &Index) -> Vec<FixCandidate> {
        let Some(tree) = TreeSitterJava::parse(source) else {
            return Vec::new();
        };
        let root = tree.root_node();
        let imports = import_nodes(root);
        if imports.len() < 2 {
            return Vec::new();
        }
        let (start, end) = (
            imports[0].start_byte(),
            line_end(source.as_bytes(), imports[imports.len() - 1].end_byte()),
        );
        let block = &source[start..end];
        if dirty_neighbours(root, start, end) {
            return Vec::new(); // comments or foreign statements inside: hands off
        }
        let lines: Vec<&str> = imports.iter().map(|n| text(*n, source)).collect();
        let target = normalised(&lines, newline_of(block));
        if target == block {
            return Vec::new();
        }
        let total = lines.len();
        vec![FixCandidate {
            rule: RULE,
            message: format!("{} imports are not in google order", total),
            severity: Severity::Info,
            auto_fixable: true,
            span: start..end,
            edits: vec![Edit {
                span: start..end,
                old_text: block.to_owned(),
                new_text: target,
            }],
            candidates: Vec::new(),
        }]
    }
}

// Top-level import statements, in source order.
fn import_nodes(root: Node) -> Vec<Node> {
    let mut cursor = root.walk();
    root.children(&mut cursor)
        .filter(|child| child.kind() == "import_declaration")
        .collect()
}

// Any root child inside the block that is not one of the imports (the
// walk stops at the statement containing the span, so comments — which
// hang off their statement — still register on the enclosing node).
fn dirty_neighbours(root: Node, start: usize, end: usize) -> bool {
    let mut cursor = root.walk();
    root.children(&mut cursor).any(|child| {
        child.kind() != "import_declaration"
            && child.start_byte() < end
            && child.end_byte() > start
            && overlaps_import_line(child, start, end)
    })
}

// The package declaration legally precedes the block; statements that
// merely abut it (zero-gap) are not inside it.
fn overlaps_import_line(child: Node, start: usize, end: usize) -> bool {
    child.start_byte() >= start && child.end_byte() <= end
}

// The sortable identity of an import: its path, without the leading
// `import [static] ` boilerplate. A free fn (not a closure) so the
// returned `&str` keeps the input's lifetime.
fn sort_key(line: &str) -> &str {
    line.strip_prefix("import static ")
        .or_else(|| line.strip_prefix("import "))
        .unwrap_or(line)
}

// Google order: statics first, blank line, then single-type imports;
// both groups ASCII-sorted.
fn normalised(lines: &[&str], nl: &str) -> String {
    let (mut statics, mut types): (Vec<&str>, Vec<&str>) =
        lines.iter().copied().partition(|l| is_static_line(l));
    statics.sort_unstable_by_key(|l| sort_key(l));
    types.sort_unstable_by_key(|l| sort_key(l));
    statics.dedup();
    types.dedup();
    let mut out: Vec<&str> = statics;
    if !out.is_empty() && !types.is_empty() {
        out.push("");
    }
    out.extend(types);
    format!("{}{nl}", out.join(nl))
}

fn newline_of(block: &str) -> &'static str {
    if block.contains("\r\n") { "\r\n" } else { "\n" }
}

#[cfg(test)]
mod tests;
