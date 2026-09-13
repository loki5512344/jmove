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
            .and_then(|decl| find_child_kind(decl, "scoped_identifier"))
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

fn text<'a>(node: Node, source: &'a str) -> &'a str {
    source
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

/// Map from every indexed Java class's declared FQN to its file. A file
/// without a `package` declaration lives in the default package, which is
/// un-importable, so it gets no entry. Collisions (two files declaring the
/// same FQN — a broken project) keep the sorted-first path deterministically.
#[derive(Debug, Default)]
pub struct JavaClassIndex {
    classes: HashMap<String, PathBuf>,
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
        Self { classes }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imports(source: &str) -> Vec<ImportRecord> {
        TreeSitterJava::new().extract_imports(source)
    }

    #[test]
    fn extracts_package_with_exact_span() {
        let src = "package com.example.utils;\n\npublic class P {}\n";
        let decl = TreeSitterJava::new().extract_package(src).expect("package");
        assert_eq!(decl.name, "com.example.utils");
        assert_eq!(&src[decl.span.clone()], "com.example.utils");
    }

    #[test]
    fn package_in_comment_is_not_captured() {
        let src = "// package com.example;\npublic class P {}\n";
        assert!(TreeSitterJava::new().extract_package(src).is_none());
    }

    #[test]
    fn extracts_single_type_and_static_imports() {
        let src = "package p;\nimport a.b.User;\nimport static a.b.User.create;\n";
        let recs = imports(src);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].specifier, "a.b.User");
        assert_eq!(&src[recs[0].span.clone()], "a.b.User");
        assert_eq!(recs[1].specifier, "a.b.User.create");
        assert_eq!(&src[recs[1].span.clone()], "a.b.User.create");
        assert!(recs.iter().all(|r| !r.is_dynamic));
    }

    #[test]
    fn on_demand_imports_are_not_extracted() {
        let src = "package p;\nimport java.util.*;\nimport a.b.User;\n";
        let recs = imports(src);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].specifier, "a.b.User");
    }

    #[test]
    fn imports_inside_nested_types_are_still_top_level() {
        let src = "package p;\nclass A { }\nimport q.B;\n";
        assert_eq!(imports(src).len(), 1);
    }

    #[test]
    fn class_index_maps_package_and_stem() {
        let mut files = FileSet::default();
        files.add(PathBuf::from("src/main/java/com/example/utils/Parser.java"));
        files.add(PathBuf::from("src/Main.java"));
        let mut packages = HashMap::new();
        packages.insert(
            PathBuf::from("src/main/java/com/example/utils/Parser.java"),
            PackageDecl {
                name: "com.example.utils".into(),
                span: 0..0,
            },
        );
        let classes = JavaClassIndex::new(&files, &packages);
        assert_eq!(
            classes.resolve("com.example.utils.Parser"),
            Some(Path::new("src/main/java/com/example/utils/Parser.java"))
        );
        // default package: un-importable, no entry
        assert_eq!(classes.classes.len(), 1);
    }

    #[test]
    fn resolve_handles_exact_and_member_imports() {
        let mut classes = HashMap::new();
        classes.insert(
            "com.example.Parser".to_owned(),
            PathBuf::from("src/com/example/Parser.java"),
        );
        let index = JavaClassIndex { classes };
        assert_eq!(
            index.resolve("com.example.Parser"),
            Some(Path::new("src/com/example/Parser.java"))
        );
        assert_eq!(
            index.resolve("com.example.Parser.parse"),
            Some(Path::new("src/com/example/Parser.java"))
        );
        assert_eq!(index.resolve("java.util.List"), None);
        assert_eq!(index.resolve("Parser"), None);
    }
}
