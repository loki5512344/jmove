//! Tree-sitter frontend for Java: `package` declaration, `import`
//! extraction and fully-qualified-name (FQN) resolution.
//!
//! CONTRACT: see [`crate::parser`]. Java specifiers are absolute FQNs
//! (`com.example.utils.Parser`), resolved through a class index built from
//! the declared packages of all indexed files — never through path
//! arithmetic. On-demand `pkg.*` imports are not extracted: a single-type
//! move never invalidates them, so rewriting them would be wrong.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser, Tree};

pub mod class_name;
pub mod rules;

use super::{ImportRecord, Language, PackageDecl, SourceLanguage};
use crate::core::index::FileSet;

/// Frontend backed by the tree-sitter Java grammar.
pub struct TreeSitterJava;

impl TreeSitterJava {
    /// Create the Java frontend.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse(source: &str) -> Option<Tree> {
        // tree-sitter's Parser holds raw pointers; build one per call
        // instead of storing it in the (Sync) frontend struct.
        let mut parser = Parser::new();
        if parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .is_err()
        {
            return None;
        }
        parser.parse(source, None)
    }
}

impl Default for TreeSitterJava {
    fn default() -> Self {
        Self::new()
    }
}

impl Language for TreeSitterJava {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Java
    }

    fn extract_imports(&self, source: &str) -> Vec<ImportRecord> {
        let Some(tree) = Self::parse(source) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut cursor = tree.root_node().walk();
        for child in tree.root_node().children(&mut cursor) {
            if child.kind() != "import_declaration" {
                continue;
            }
            // `import pkg.*;` — the dotted path never names one file.
            if has_child_kind(child, "asterisk") {
                continue;
            }
            // The grammar exposes no field names on import/package nodes;
            // the dotted path is the single `scoped_identifier` child.
            if let Some(path) = find_child_kind(child, "scoped_identifier") {
                out.push(record(path, source));
            }
        }
        out
    }

    fn extract_package(&self, source: &str) -> Option<PackageDecl> {
        let tree = Self::parse(source)?;
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .find(|child| child.kind() == "package_declaration")
            // A dotted name parses as `scoped_identifier`; a single-segment
            // package (`package p;`) has no dots and is a bare `identifier`.
            .and_then(|decl| {
                find_child_kind(decl, "scoped_identifier")
                    .or_else(|| find_child_kind(decl, "identifier"))
            })
            .map(|name| PackageDecl {
                name: text(name, source).to_owned(),
                span: name.start_byte()..name.end_byte(),
            })
    }
}

fn has_child_kind(node: Node, kind: &str) -> bool {
    find_child_kind(node, kind).is_some()
}

fn find_child_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

// Specifier text of a `scoped_identifier` node (no quotes in Java).
fn record(node: Node, source: &str) -> ImportRecord {
    ImportRecord {
        specifier: text(node, source).to_owned(),
        span: node.start_byte()..node.end_byte(),
        is_dynamic: false,
    }
}

fn text<'a>(node: Node<'a>, source: &'a str) -> &'a str {
    source
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

// Extend a statement end over exactly one `\n` or `\r\n` line terminator.
pub(super) fn line_end(source: &[u8], end: usize) -> usize {
    match (source.get(end), source.get(end + 1)) {
        (Some(b'\r'), Some(b'\n')) => end + 2,
        (Some(b'\n'), _) => end + 1,
        _ => end,
    }
}

/// Map from every indexed Java class's declared FQN to its file. A file
/// without a `package` declaration lives in the default package, which is
/// un-importable, so it gets no entry. Collisions (two files declaring the
/// same FQN — a broken project) keep the sorted-first path deterministically.
#[derive(Debug, Default)]
pub struct JavaClassIndex {
    classes: HashMap<String, PathBuf>,
    // simple name -> sorted FQNs declaring it (missing-import lookups).
    by_simple: HashMap<String, Vec<String>>,
}

impl JavaClassIndex {
    /// Build the index from the scanned files and their package declarations.
    #[must_use]
    pub fn new(files: &FileSet, packages: &HashMap<PathBuf, PackageDecl>) -> Self {
        let mut classes: HashMap<String, PathBuf> = HashMap::new();
        for file in files.sorted() {
            let Some(decl) = packages.get(&file) else {
                continue;
            };
            let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            classes
                .entry(format!("{}.{stem}", decl.name))
                .or_insert(file);
        }
        let mut by_simple: HashMap<String, Vec<String>> = HashMap::new();
        for fqn in classes.keys() {
            let simple = fqn.rsplit('.').next().unwrap_or(fqn);
            by_simple
                .entry(simple.to_owned())
                .or_default()
                .push(fqn.clone());
        }
        for fqns in by_simple.values_mut() {
            fqns.sort();
        }
        Self { classes, by_simple }
    }

    /// Resolve a Java import `specifier` to an indexed file. Exact FQN
    /// first; failing that, drop the last segment once so `import static
    /// com.example.Parser.parse` (a member import) lands on
    /// `com.example.Parser`. Anything left unresolved is an external
    /// (jdk/third-party) import.
    #[must_use]
    pub fn resolve(&self, specifier: &str) -> Option<&Path> {
        if let Some(file) = self.classes.get(specifier) {
            return Some(file.as_path());
        }
        let owner = specifier.rsplit_once('.')?.0;
        self.classes.get(owner).map(PathBuf::as_path)
    }

    /// Every indexed FQN whose simple name is `simple`, sorted. Empty for
    /// jdk/third-party names, which the index never contains.
    #[must_use]
    pub fn candidates(&self, simple: &str) -> &[String] {
        self.by_simple.get(simple).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests;
