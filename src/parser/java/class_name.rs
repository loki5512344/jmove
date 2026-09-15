//! Java layout check: the single public top-level type must match the
//! file name (javac: "class Foo is public, should be declared in a file
//! named Foo.java").
//!
//! This is a *finding*, not a fix rule: the repair is a file rename, and
//! the fix engine applies byte edits only. `jmove check` reports it with
//! the exact `jmove mv` command that repairs the layout — the moved class
//! keeps its FQN, so renaming the file rewrites no imports.

use std::ops::Range;
use std::path::Path;

use tree_sitter::Node;

use super::{TreeSitterJava, text};

/// Kinds that introduce a top-level type in Java.
const TYPE_KINDS: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

/// The mismatch details: the public type's name, the span of its name
/// token (for line reporting) and the file stem it should live in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// Name of the single public top-level type.
    pub public_class: String,
    /// Byte span of the class name token.
    pub span: Range<usize>,
}

/// `Some` when the file declares exactly one public top-level type whose
/// name differs from the file stem. Two public types (also illegal) or
/// zero are not this check's business. `package-info`/`module-info` files
/// never name a type and are skipped.
#[must_use]
pub fn mismatch(path: &Path, source: &str) -> Option<Mismatch> {
    let stem = path.file_stem()?.to_str()?;
    if matches!(stem, "package-info" | "module-info") {
        return None;
    }
    let tree = TreeSitterJava::parse(source)?;
    let mut cursor = tree.root_node().walk();
    let publics: Vec<Node> = tree
        .root_node()
        .children(&mut cursor)
        .filter(|n| TYPE_KINDS.contains(&n.kind()))
        .filter(|n| has_public_modifier(*n))
        .collect();
    let [only] = publics.as_slice() else {
        return None; // zero or many public types: not a naming mismatch
    };
    let mut c = only.walk();
    let name = only.children(&mut c).find(|n| n.kind() == "identifier")?;
    let name_text = text(name, source);
    (name_text != stem).then(|| Mismatch {
        public_class: name_text.to_owned(),
        span: name.byte_range(),
    })
}

// `public` is an anonymous token inside the `modifiers` child.
fn has_public_modifier(node: Node) -> bool {
    let mut c = node.walk();
    node.children(&mut c)
        .find(|n| n.kind() == "modifiers")
        .is_some_and(|mods| {
            let mut m = mods.walk();
            mods.children(&mut m).any(|t| t.kind() == "public")
        })
}

#[cfg(test)]
mod tests {
    use super::mismatch;
    use std::path::Path;

    fn m(path: &str, src: &str) -> Option<String> {
        mismatch(Path::new(path), src).map(|f| f.public_class)
    }

    #[test]
    fn public_type_name_must_equal_file_stem() {
        assert_eq!(
            m("Foo.java", "package p;\npublic class Bar {}\n").as_deref(),
            Some("Bar")
        );
        assert_eq!(m("Foo.java", "package p;\npublic class Foo {}\n"), None);
    }

    #[test]
    fn every_public_top_level_kind_is_checked() {
        assert_eq!(
            m("A.java", "public interface B { int x(); }").as_deref(),
            Some("B")
        );
        assert_eq!(m("A.java", "public enum B { X }").as_deref(), Some("B"));
        assert_eq!(
            m("A.java", "public record B(int x) {}").as_deref(),
            Some("B")
        );
        assert_eq!(m("A.java", "public @interface B {}").as_deref(), Some("B"));
    }

    #[test]
    fn non_public_multiple_or_nested_types_are_not_reported() {
        // Zero public types: legal package-private layout.
        assert_eq!(m("Foo.java", "class Bar {}\nclass Baz {}"), None);
        // Two public top-level types: a different (harder) error.
        assert_eq!(m("Foo.java", "public class A {}\npublic class B {}"), None);
        // Nested publics live inside a type: not top-level.
        assert_eq!(m("A.java", "class A { public class B {} }"), None);
        // Protected/private modifiers do not trigger the javac rule.
        assert_eq!(m("A.java", "class B {}"), None);
    }

    #[test]
    fn info_files_are_skipped_and_span_names_the_class_token() {
        assert_eq!(m("package-info.java", "package p;"), None);
        let src = "package p;\n\npublic class Bar {}\n";
        let found = mismatch(Path::new("Foo.java"), src).unwrap();
        assert_eq!(&src[found.span.clone()], "Bar");
    }
}
