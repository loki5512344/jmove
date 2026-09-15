//! Tests for the `java/missing-import` rule (child module: sees the
//! rule's private helpers through `super`).

use super::JavaMissingImports;
use crate::core::index::Index;
use crate::parser::Fix;
use std::fs;
use std::path::Path;

// Index built from `rel -> body` files; the tempdir guard must stay
// alive as long as the returned index.
fn index_of(files: &[(&str, &str)]) -> (tempfile::TempDir, Index) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    for (rel, body) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        fs::write(path, body).expect("write");
    }
    let index = Index::build(dir.path()).expect("index");
    (dir, index)
}

fn fix(rel: &str, source: &str, index: &Index) -> Vec<crate::parser::FixCandidate> {
    JavaMissingImports::new().fixes(Path::new(rel), source, index)
}

const G_CLASS: &str = "package a.b;\n\npublic class G {}\n";

#[test]
fn unique_candidate_gets_an_inserted_import_after_the_package() {
    let (_dir, index) = index_of(&[("src/a/b/G.java", G_CLASS)]);
    let src = "package p;\n\nclass C { G g; }\n";
    let found = fix("src/p/C.java", src, &index);
    assert_eq!(found.len(), 1);
    assert!(found[0].auto_fixable);
    assert_eq!(found[0].edits[0].new_text, "import a.b.G;\n");
    assert_eq!(
        found[0].edits[0].span,
        src.find("\n\nclass").unwrap() + 1..src.find("\n\nclass").unwrap() + 1
    );
    assert_eq!(found[0].message, "missing import 'a.b.G' for type 'G'");
}

#[test]
fn insertion_lands_after_the_last_existing_import() {
    let (_dir, index) = index_of(&[("src/a/b/G.java", G_CLASS)]);
    let src = "package p;\nimport x.Y;\n\nclass C { G g; Y y; }\n";
    let found = fix("src/p/C.java", src, &index);
    assert_eq!(found.len(), 1);
    assert_eq!(
        &src[..found[0].edits[0].span.start],
        "package p;\nimport x.Y;\n"
    );
}

#[test]
fn same_package_declared_imported_and_jdk_names_never_fix() {
    let (_dir, index) = index_of(&[
        ("src/a/b/G.java", G_CLASS),
        (
            "src/p/Sibling.java",
            "package p;\npublic class Sibling {}\n",
        ),
    ]);
    // Same-package sibling: resolves without an import.
    let src = "package p;\nclass C { Sibling s; }\n";
    assert!(fix("src/p/C.java", src, &index).is_empty());
    // Declared in the file itself, already imported, lowercase method:
    // none of them is a missing type.
    let src = "package p;\nimport a.b.G;\nclass C { G g; H h; void g() {} }\nclass H {}\n";
    assert!(fix("src/p/C.java", src, &index).is_empty());
    // `java.util.List`: not in the class index at all.
    let src = "package p;\nimport java.util.List;\nclass C { List l; }\n";
    assert!(fix("src/p/C.java", src, &index).is_empty());
}

#[test]
fn ambiguous_and_wildcard_findings_are_manual_with_candidates() {
    let (_dir, index) = index_of(&[
        ("src/a/b/G.java", G_CLASS),
        ("src/z/G.java", "package z;\npublic class G {}\n"),
        ("src/u/U.java", "package u;\npublic class U {}\n"),
    ]);
    // Two indexed `G`s: the agent must choose.
    let src = "package p;\nclass C { G g; }\n";
    let found = fix("src/p/C.java", src, &index);
    assert_eq!(found.len(), 1);
    assert!(!found[0].auto_fixable);
    assert!(found[0].edits.is_empty());
    assert_eq!(found[0].candidates, ["a.b.G".to_owned(), "z.G".to_owned()]);
    // A unique name is still manual when a `pkg.*` may shadow it.
    let src = "package p;\nimport q.*;\nclass C { U u; }\n";
    let found = fix("src/p/C.java", src, &index);
    assert_eq!(found.len(), 1);
    assert!(!found[0].auto_fixable);
    assert_eq!(found[0].candidates, ["u.U".to_owned()]);
}

#[test]
fn annotations_static_uses_and_crlf_stay_bare_references() {
    let (_dir, index) = index_of(&[("src/a/b/G.java", G_CLASS)]);
    let src = "package p;\r\n\r\nclass C {\r\n @G void m() { G.make(); }\r\n}\r\n";
    let found = fix("src/p/C.java", src, &index);
    assert_eq!(found.len(), 1, "one candidate per name, not per use");
    assert!(found[0].auto_fixable);
    // Qualified uses need no import: the dotted chain is skipped.
    let src = "package p;\nclass C { a.b.G g; }\n";
    assert!(fix("src/p/C.java", src, &index).is_empty());
}

#[test]
fn moved_file_gets_its_sibling_import_like_guava() {
    // The guava failure shape: `H` moved out of `a.b`, still referring
    // to sibling `G` bare. Index says G lives in a.b exactly once.
    let (_dir, index) = index_of(&[("src/a/b/G.java", G_CLASS)]);
    let src = "package a.c;\n\nclass H { G wrap() { return null; } }\n";
    let found = fix("src/a/c/H.java", src, &index);
    assert_eq!(found.len(), 1);
    assert!(found[0].auto_fixable);
    assert_eq!(found[0].edits[0].new_text, "import a.b.G;\n");
}
