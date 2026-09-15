//! Deterministic Java fix rules (`java/unused-import`, `java/missing-import`,
//! `java/import-order`). Each rule implements
//! [`Fix`](crate::parser::Fix) and reports candidates for the shared
//! dry-run/atomic engine; frontend helpers live in [`super`].

pub mod import_order;
pub mod missing_imports;
pub mod unused_imports;

use tree_sitter::Node;

use crate::parser::java::text;

/// `import static ...;` — the grammar keeps `static` as an anonymous token,
/// so the statement text is the cheapest reliable check.
#[must_use]
pub(super) fn is_static(node: Node, source: &str) -> bool {
    is_static_line(text(node, source))
}

/// The same check on already-extracted statement text.
#[must_use]
pub(super) fn is_static_line(line: &str) -> bool {
    line.starts_with("import static ")
}
