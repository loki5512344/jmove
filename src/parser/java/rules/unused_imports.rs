//! Java rule: drop single-type imports whose simple name is never used.
//!
//! Detection is a raw word-boundary scan over the whole file (comments and
//! strings included), never an AST usage analysis. That asymmetry is the
//! safety property: a false "still used" verdict keeps one dead import
//! (harmless), while a wrong "unused" verdict deletes live code. So a type
//! referenced only in javadoc or a string literal keeps its import, and an
//! import whose name appears in a *sibling import* (e.g. the static member
//! import of the same type) is kept too.
//!
//! The deletion edit covers the whole statement plus its line ending
//! (CRLF-aware); leading indentation is not absorbed — Java imports sit at
//! column zero, and anything else is the generator's problem, not the
//! engine's (see [`crate::core::Edit`]).

use std::ops::Range;
use std::path::Path;

use tree_sitter::Node;

use super::is_static;
use crate::core::Edit;
use crate::core::index::Index;
use crate::parser::java::{TreeSitterJava, find_child_kind, has_child_kind, line_end, text};
use crate::parser::{Fix, FixCandidate, Severity};

/// Rule id accepted by `jmove fix --rule`.
pub const RULE: &str = "java/unused-import";

/// Unused single-type import remover.
pub struct JavaUnusedImports;

impl JavaUnusedImports {
    /// Create the rule.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for JavaUnusedImports {
    fn default() -> Self {
        Self::new()
    }
}

impl Fix for JavaUnusedImports {
    fn rule(&self) -> &'static str {
        RULE
    }

    fn fixes(&self, _path: &Path, source: &str, _index: &Index) -> Vec<FixCandidate> {
        let Some(tree) = TreeSitterJava::parse(source) else {
            return Vec::new();
        };
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .filter(|node| node.kind() == "import_declaration")
            .filter(|node| !has_child_kind(*node, "asterisk")) // `pkg.*`
            .filter_map(|node| unused_import(node, source))
            .collect()
    }
}

// One candidate when the import's used-name(s) occur nowhere else.
fn unused_import(node: Node, source: &str) -> Option<FixCandidate> {
    let path = find_child_kind(node, "scoped_identifier")?;
    let specifier = text(path, source);
    let names: Vec<&str> = if is_static(node, source) {
        // `a.b.User.create` is used via `create(...)` or `User.create(...)`.
        let mut segments = specifier.rsplitn(2, '.');
        let member = segments.next()?;
        let owner = segments.next()?.rsplit('.').next()?;
        vec![member, owner]
    } else {
        vec![specifier.rsplit('.').next()?]
    };
    let skip = node.start_byte()..node.end_byte();
    if names
        .iter()
        .any(|name| word_occurs(source.as_bytes(), name.as_bytes(), &skip))
    {
        return None;
    }
    let span = skip.start..line_end(source.as_bytes(), skip.end);
    let old_text = source[span.clone()].to_owned();
    Some(FixCandidate {
        rule: RULE,
        message: format!("unused import '{specifier}'"),
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

// `word` as a standalone Java identifier token outside `skip`. Matches
// overlapping the import statement itself never count as usage. Bytes
// >= 0x80 count as identifier parts: treating a possibly-mojibake
// neighbour as "part of a bigger word" can only keep an import, never
// drop one.
fn word_occurs(source: &[u8], word: &[u8], skip: &Range<usize>) -> bool {
    if word.is_empty() {
        return false;
    }
    source.windows(word.len()).enumerate().any(|(at, found)| {
        let end = at + word.len();
        if at < skip.end && end > skip.start {
            return false;
        }
        let before = at == 0 || !is_ident(source[at - 1]);
        let after = end == source.len() || !is_ident(source[end]);
        *found == *word && before && after
    })
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use super::{JavaUnusedImports, RULE};
    use crate::core::index::Index;
    use crate::parser::{Fix, Severity};
    use std::path::Path;

    fn candidates(source: &str) -> Vec<crate::parser::FixCandidate> {
        JavaUnusedImports::new().fixes(Path::new("A.java"), source, &Index::default())
    }

    #[test]
    fn unused_import_is_deleted_with_its_line() {
        let src = "package p;\n\nimport a.b.User;\n\nclass C { int u; }\n";
        let found = candidates(src);
        assert_eq!(found.len(), 1);
        let candidate = &found[0];
        assert_eq!(candidate.rule, RULE);
        assert!(candidate.auto_fixable);
        assert_eq!(candidate.severity, Severity::Warning);
        assert_eq!(candidate.message, "unused import 'a.b.User'");
        let edit = &candidate.edits[0];
        assert_eq!(edit.new_text, "");
        assert_eq!(&src[edit.span.clone()], "import a.b.User;\n");
    }

    #[test]
    fn used_types_annotations_and_words_are_kept() {
        let src = "import a.b.User;\nclass C implements User { @User Ann u; }";
        assert!(candidates(src).is_empty());
        // `UserFactory` is a different token: it does not keep `User`.
        let src = "import a.b.User;\nclass C { UserFactory f; }";
        assert_eq!(candidates(src).len(), 1);
        // A javadoc/string mention keeps the import (safe direction).
        let src = "import a.b.User;\n/** see {@link User} */ class C {}";
        assert!(candidates(src).is_empty());
    }

    #[test]
    fn static_imports_check_member_and_owner_names() {
        // `create` used directly.
        let src = "import static a.b.User.create;\nclass C { auto x = create(); }";
        assert!(candidates(src).is_empty());
        // `User` used as the qualifier.
        let src = "import static a.b.User.create;\nclass C { auto x = User.create(); }";
        assert!(candidates(src).is_empty());
        // neither appears: the import goes.
        let src = "import static a.b.User.create;\nclass C {}";
        assert_eq!(candidates(src).len(), 1);
    }

    #[test]
    fn sibling_import_mention_keeps_the_type_import() {
        let src = "import a.b.User;\nimport static a.b.User.create;\nclass C {}";
        // Each statement sees the other's `User`; neither is provably dead.
        assert!(candidates(src).is_empty());
    }

    #[test]
    fn wildcards_and_crlf_are_handled() {
        let src = "import a.b.*;\nclass C {}";
        assert!(candidates(src).is_empty());
        let src = "import a.b.User;\r\nclass C {}";
        let found = candidates(src);
        assert_eq!(found.len(), 1);
        assert_eq!(&src[found[0].edits[0].span.clone()], "import a.b.User;\r\n");
    }
}
