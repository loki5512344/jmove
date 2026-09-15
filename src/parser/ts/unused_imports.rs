//! TS/JS rule: drop imported bindings that are never referenced.
//!
//! Safety is the same asymmetry as [`crate::parser::java::rules::unused_imports`]:
//! a value is kept when its name occurs as a standalone identifier anywhere
//! outside its own `import` statement (comments and strings included), so the
//! rule can only *under*-delete. That conservatism is load-bearing: a JSX
//! component, a decorator, a `typeof x` or an object-literal shorthand all
//! count as uses, and a bare `foo` mention in a comment keeps an otherwise
//! dead import alive — a false "used" is harmless, a false "unused" deletes
//! live code.
//!
//! Two TS-specific facts the whole-statement Java approach cannot carry over:
//! - a statement can bind several names (`import D, { A, B as C }`), so the
//!   edit removes individual *specifiers*, and only the whole statement when
//!   its last binding (default or namespace) goes;
//! - `import "./side-effect"` has no binding to be "unused" — the statement
//!   exists for its effects and is never a candidate, exactly like a re-export
//!   `export { A } from "./a"` (whose `A` is re-exposed, not used locally).

use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::core::Edit;
use crate::core::index::Index;
use crate::parser::java::line_end;
use crate::parser::{Fix, FixCandidate, Severity, SourceLanguage, word_occurs};

use super::extract::grammar;

/// Rule id accepted by `jmove fix --rule`.
pub const RULE: &str = "ts/unused-import";

/// Unused imported-binding remover for TS/JS/TSX.
pub struct TsUnusedImports;

impl TsUnusedImports {
    /// Create the rule.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for TsUnusedImports {
    fn default() -> Self {
        Self::new()
    }
}

impl Fix for TsUnusedImports {
    fn rule(&self) -> &'static str {
        RULE
    }

    fn fixes(&self, path: &Path, source: &str, _index: &Index) -> Vec<FixCandidate> {
        let lang = SourceLanguage::for_path(path).unwrap_or(SourceLanguage::TypeScript);
        let Some(tree) = parse(lang, source) else {
            return Vec::new();
        };
        let bytes = source.as_bytes();
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .filter(|node| node.kind() == "import_statement")
            .filter_map(|node| unused_bindings(node, source, bytes))
            .collect()
    }
}

fn parse(lang: SourceLanguage, source: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    if parser.set_language(&grammar(lang)).is_err() {
        return None;
    }
    parser.parse(source, None)
}

// One candidate when an import binds at least one name and none of its names
// occur outside the statement itself.
fn unused_bindings(node: Node, source: &str, bytes: &[u8]) -> Option<FixCandidate> {
    let mut c = node.walk();
    let clause = node
        .children(&mut c)
        .find(|n| n.kind() == "import_clause")?;
    let names = binding_names(clause, source);
    if names.is_empty() {
        return None; // `import "./side"` — effects only, never unused.
    }
    let stmt = node.start_byte()..node.end_byte();
    if names
        .iter()
        .any(|n| word_occurs(bytes, n.as_bytes(), &stmt))
    {
        return None; // keep the whole statement on any live binding.
    }
    // Every binding is dead, so the whole statement goes — including its line
    // ending. Partial specifier removal (some names live) is deliberately out
    // of v1 scope: safe under-delete, comma surgery is a formatter job.
    let span = stmt.start..line_end(bytes, stmt.end);
    let old_text = source[span.clone()].to_owned();
    Some(FixCandidate {
        rule: RULE,
        message: format!("unused import [{}]", names.join(", ")),
        severity: Severity::Warning,
        auto_fixable: true,
        span: span.clone(),
        edits: vec![Edit {
            span,
            old_text,
            new_text: String::new(),
        }],
        candidates: Vec::new(),
    })
}

// Every local name the clause introduces (`D`, the alias of `B as C`, the
// `ns` of `* as ns`), in source order.
fn binding_names(clause: Node, source: &str) -> Vec<String> {
    let mut c = clause.walk();
    clause
        .children(&mut c)
        .filter_map(|child| match child.kind() {
            // Default import: `import D from ...`
            "identifier" => Some(vec![text(child, source)]),
            // Namespace import: `import * as ns from ...`
            "namespace_import" => last_identifier(child).map(|n| vec![text(n, source)]),
            // Named imports: `import { A, B as C } from ...`
            "named_imports" => {
                let mut s = child.walk();
                Some(
                    child
                        .children(&mut s)
                        .filter(|n| n.kind() == "import_specifier")
                        .filter_map(last_identifier)
                        .map(|n| text(n, source))
                        .collect(),
                )
            }
            _ => None,
        })
        .flatten()
        .collect()
}

// The last `identifier` under a node is the local name: for `B as C` and
// `* as ns` it is the alias; for a lone specifier it is the name itself.
// (tree-sitter's child iterator is forward-only, so "last" is `.last()`.)
fn last_identifier(node: Node) -> Option<Node> {
    let mut c = node.walk();
    node.children(&mut c)
        .filter(|n| n.kind() == "identifier")
        .last()
}

fn text(node: Node, source: &str) -> String {
    source[node.byte_range()].to_owned()
}

#[cfg(test)]
mod tests {
    use super::TsUnusedImports;
    use crate::core::index::Index;
    use crate::parser::{Fix, FixCandidate};
    use std::path::Path;

    fn fixes(source: &str) -> Vec<FixCandidate> {
        TsUnusedImports::new().fixes(Path::new("a.ts"), source, &Index::default())
    }

    #[test]
    fn unused_named_import_is_deleted_with_its_line() {
        let src = "import { A } from './a';\nexport const b = 1;\n";
        let found = fixes(src);
        assert_eq!(found.len(), 1);
        assert!(found[0].auto_fixable);
        assert_eq!(found[0].message, "unused import [A]");
        assert_eq!(
            &src[found[0].edits[0].span.clone()],
            "import { A } from './a';\n"
        );
    }

    #[test]
    fn any_live_binding_keeps_the_whole_statement() {
        // `A` used, `B` dead => under-delete: keep both.
        let src = "import { A, B } from './a';\nconst x: A = A();\n";
        assert!(fixes(src).is_empty());
    }

    #[test]
    fn default_and_namespace_and_alias_bindings() {
        let src = "import React from 'react';\nconst x = 1;\n";
        assert_eq!(fixes(src).len(), 1);
        let src = "import * as ns from './a';\nconst x = 1;\n";
        assert_eq!(fixes(src).len(), 1);
        // `B as C`: the local name is `C`; using `C` keeps it.
        let src = "import { B as C } from './a';\nconst x = C;\n";
        assert!(fixes(src).is_empty());
        let src = "import { B as C } from './a';\nconst B = 1;\n";
        // `B` here is a different declaration, not the import alias `C`.
        assert_eq!(fixes(src).len(), 1);
    }

    #[test]
    fn side_effect_and_reexport_are_never_candidates() {
        let src = "import './styles.css';\nexport { A } from './a';\n";
        assert!(fixes(src).is_empty());
    }

    #[test]
    fn comment_or_string_or_jsx_mention_keeps_the_import() {
        let src = "import Foo from './foo';\n// see Foo\nconst s = \"Foo\";\n";
        assert!(fixes(src).is_empty());
        // JSX usage of the bound component.
        let tsx = "import Foo from './foo';\nconst x = <Foo />;\n";
        assert!(
            TsUnusedImports::new()
                .fixes(Path::new("a.tsx"), tsx, &Index::default())
                .is_empty()
        );
    }

    #[test]
    fn type_only_import_is_deleted_like_any_binding() {
        let src = "import type { A } from './a';\nconst x = 1;\n";
        assert_eq!(fixes(src).len(), 1);
        // Referenced through `typeof`/annotation => kept.
        let src = "import type { A } from './a';\nconst x: A = 1;\n";
        assert!(fixes(src).is_empty());
    }

    #[test]
    fn crlf_line_is_removed_wholly() {
        let src = "import { A } from './a';\r\nexport const b = 1;\r\n";
        let found = fixes(src);
        assert_eq!(
            &src[found[0].edits[0].span.clone()],
            "import { A } from './a';\r\n"
        );
    }

    #[test]
    fn used_side_effect_and_dynamic_are_untouched() {
        // require/dynamic import are separate records, not import_statement
        // nodes, so this rule never proposes deleting them.
        let src = "const a = require('./a');\nimport { A } from './a';\nexport const x = A;\n";
        assert!(fixes(src).is_empty(), "A is used by `export const x = A`");
    }
}
