//! Java rule: add the missing `import` of a project class that is used by
//! its simple name.
//!
//! Motivation (guava smoke test): after `mv` moves a `.java` file into a
//! new package, its unqualified references to former same-package siblings
//! stop resolving — the compiler needs an explicit import. The rule adds
//! one only when the FQN index proves a single candidate; ambiguity is
//! reported as non-auto `candidates` for an agent to resolve, never
//! guessed at.
//!
//! Safety mirrors [`super::super::unused_imports`] from the other side: adding an
//! import can only be harmless-or-wrong, so the wrong case is fenced off
//! structurally — dotted chains (`a.b.Foo`) and files with on-demand
//! imports never auto-fix, same-package siblings are skipped (they resolve
//! without an import), and names declared in or imported by the file are
//! invisible to the rule. Comments and strings produce no AST nodes, so a
//! mention there cannot trigger an insertion.

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

use tree_sitter::Node;

use crate::core::Edit;
use crate::core::index::Index;
use crate::parser::java::{TreeSitterJava, find_child_kind, has_child_kind, line_end, text};
use crate::parser::{Fix, FixCandidate, Severity};

/// Rule id accepted by `jmove fix --rule`.
pub const RULE: &str = "java/missing-import";

/// Missing-import adder for Java sources.
pub struct JavaMissingImports;

impl JavaMissingImports {
    /// Create the rule.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for JavaMissingImports {
    fn default() -> Self {
        Self::new()
    }
}

// What the file already makes resolvable without a new import, plus the
// insertion anchor for the import block.
#[derive(Default)]
struct Scope {
    declared: HashSet<String>,
    imported: HashSet<String>,
    wildcard: bool,
    last_import: usize,
    package_line: usize,
    package_name: Option<String>,
}

impl Fix for JavaMissingImports {
    fn rule(&self) -> &'static str {
        RULE
    }

    fn fixes(&self, _path: &Path, source: &str, index: &Index) -> Vec<FixCandidate> {
        let Some(tree) = TreeSitterJava::parse(source) else {
            return Vec::new();
        };
        let mut scope = Scope::default();
        collect_scope(tree.root_node(), source, &mut scope);
        let mut refs: Vec<(String, Range<usize>)> = Vec::new();
        collect_refs(tree.root_node(), source, &scope, &mut refs);
        // After the last import; else after the package line; else at the
        // head of the file. Insertions at one byte keep candidate order.
        let anchor = scope.last_import.max(scope.package_line);
        refs.iter()
            .filter_map(|(name, span)| candidate(name, span, anchor, &scope, index))
            .collect()
    }
}

// One candidate per unresolved name: an auto insertion when the index
// proves a unique importable FQN, else a manual report for the agent.
fn candidate(
    name: &str,
    span: &Range<usize>,
    anchor: usize,
    scope: &Scope,
    index: &Index,
) -> Option<FixCandidate> {
    let cands = index.java_classes.candidates(name);
    if cands.is_empty() {
        return None; // jdk/third-party name: the index cannot invent an import
    }
    // The file's own declared package wins over the index: `mv` may have
    // rewritten it after the index snapshot, and the source is the truth.
    if scope
        .package_name
        .as_ref()
        .is_some_and(|pkg| cands.iter().any(|fqn| fqn == &format!("{pkg}.{name}")))
    {
        return None; // same-package sibling resolves without an import
    }
    if cands.len() == 1 && !scope.wildcard {
        let fqn = &cands[0];
        return Some(FixCandidate {
            rule: RULE,
            message: format!("missing import '{fqn}' for type '{name}'"),
            severity: Severity::Error,
            auto_fixable: true,
            span: span.clone(),
            edits: vec![Edit {
                span: anchor..anchor,
                old_text: String::new(),
                new_text: format!("import {fqn};\n"),
            }],
            candidates: Vec::new(),
        });
    }
    let why = if scope.wildcard {
        "on-demand imports may already resolve it"
    } else {
        "the index holds several classes with this name"
    };
    Some(FixCandidate {
        rule: RULE,
        message: format!(
            "type '{name}' needs an import: {why} ({})",
            cands.join(", ")
        ),
        severity: Severity::Error,
        auto_fixable: false,
        span: span.clone(),
        edits: Vec::new(),
        candidates: cands.to_vec(),
    })
}

// Pass 1: the names the file already resolves, and the import-block end.
fn collect_scope(node: Node, source: &str, scope: &mut Scope) {
    match node.kind() {
        "import_declaration" => {
            if has_child_kind(node, "asterisk") {
                scope.wildcard = true;
            } else if let Some(spec) = find_child_kind(node, "scoped_identifier")
                .or_else(|| find_child_kind(node, "identifier"))
            {
                let simple = text(spec, source).rsplit('.').next().unwrap_or_default();
                scope.imported.insert(simple.to_owned());
            }
            scope.last_import = line_end(source.as_bytes(), node.end_byte());
        }
        "package_declaration" => {
            scope.package_line = line_end(source.as_bytes(), node.end_byte());
            scope.package_name = find_child_kind(node, "scoped_identifier")
                .or_else(|| find_child_kind(node, "identifier"))
                .map(|n| text(n, source).to_owned());
        }
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration"
        | "type_parameter" => {
            if let Some(name) = declared_name(node, source) {
                scope.declared.insert(name);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_scope(child, source, scope);
    }
}

// The declaration's own name node (`type_parameter` keeps it unnamed).
fn declared_name(node: Node, source: &str) -> Option<String> {
    let name = node
        .child_by_field_name("name")
        .or_else(|| find_child_kind(node, "type_identifier"))?;
    Some(text(name, source).to_owned())
}

// Positions whose `name` field is a member/declarator, not a type use.
const MEMBER_NAME: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "variable_declarator",
    "method_invocation",
    "field_access",
    "enum_constant",
];

// Pass 2: bare capitalized identifier/type_identifier occurrences.
// Dotted chains, package and import statements resolve on their own and
// are never descended into.
fn collect_refs(node: Node, source: &str, scope: &Scope, refs: &mut Vec<(String, Range<usize>)>) {
    match node.kind() {
        "import_declaration"
        | "package_declaration"
        | "scoped_identifier"
        | "scoped_type_identifier" => return,
        "identifier" | "type_identifier" if !is_member_name(node) => {
            push_ref(text(node, source), node.byte_range(), scope, refs);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_refs(child, source, scope, refs);
    }
}

fn is_member_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !MEMBER_NAME.contains(&parent.kind()) {
        return false;
    }
    parent
        .child_by_field_name("name")
        .is_some_and(|n| n.start_byte() == node.start_byte() && n.end_byte() == node.end_byte())
}

// Keep the first occurrence of each new capitalized, unresolved name.
fn push_ref(name: &str, span: Range<usize>, scope: &Scope, refs: &mut Vec<(String, Range<usize>)>) {
    if !name.chars().next().is_some_and(char::is_uppercase)
        || scope.declared.contains(name)
        || scope.imported.contains(name)
        || refs.iter().any(|(seen, _)| seen == name)
    {
        return;
    }
    refs.push((name.to_owned(), span));
}

#[cfg(test)]
mod tests;
