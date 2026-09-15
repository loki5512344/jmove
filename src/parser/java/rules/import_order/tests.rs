use super::JavaImportOrder;
use crate::core::index::Index;
use crate::parser::Fix;
use std::path::Path;

fn fix(source: &str) -> Vec<crate::parser::FixCandidate> {
    JavaImportOrder::new().fixes(Path::new("A.java"), source, &Index::default())
}

#[test]
fn statics_move_first_and_groups_sort_by_ascii() {
    let src = "package p;\n\nimport java.util.List;\nimport com.example.Text;\nimport static java.lang.Math.PI;\nimport java.util.Map;\n\nclass C {}\n";
    let found = fix(src);
    assert_eq!(found.len(), 1);
    assert!(found[0].auto_fixable);
    assert_eq!(found[0].severity, crate::parser::Severity::Info);
    let edit = &found[0].edits[0];
    assert_eq!(
        edit.new_text,
        "import static java.lang.Math.PI;\n\nimport com.example.Text;\nimport java.util.List;\nimport java.util.Map;\n"
    );
    // The replace span is exactly the old block: the staleness guard.
    assert_eq!(&src[edit.span.clone()], edit.old_text);
}

#[test]
fn sorted_and_single_blocks_produce_nothing() {
    let src = "import static a.A;\n\nimport a.B;\nimport b.C;\n\nclass D {}\n";
    assert!(fix(src).is_empty());
    // One import can never be out of order.
    assert!(fix("import a.B;\nclass D {}\n").is_empty());
}

#[test]
fn duplicates_collapse_and_only_dupes_change_the_block() {
    let src = "import a.B;\nimport a.B;\nimport a.C;\n\nclass D {}\n";
    let found = fix(src);
    assert_eq!(found.len(), 1, "dedup alone is a change");
    assert_eq!(found[0].edits[0].new_text, "import a.B;\nimport a.C;\n");
}

#[test]
fn comments_and_foreign_statements_abort_the_rewrite() {
    // Comment attached inside the block: re-sorting would orphan it.
    let src = "import b.B;\n// keep first\nimport a.A;\n\nclass D {}\n";
    assert!(fix(src).is_empty());
    // A type declaration wedged between imports: not a clean block.
    let src = "import b.B;\nclass Mid {}\nimport a.A;\n";
    assert!(fix(src).is_empty());
}

#[test]
fn crlf_blocks_stay_crlf() {
    let src = "import b.B;\r\nimport a.A;\r\n\r\nclass D {}\r\n";
    let found = fix(src);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].edits[0].new_text, "import a.A;\r\nimport b.B;\r\n");
}

#[test]
fn sorting_applies_to_the_whole_statement_after_the_keyword() {
    // `java` sorts before `javax` by raw ASCII, matching Checkstyle.
    let src = "import javax.swing.JDialog;\nimport java.util.List;\n\nclass D {}\n";
    let found = fix(src);
    assert_eq!(
        found[0].edits[0].new_text,
        "import java.util.List;\nimport javax.swing.JDialog;\n"
    );
}

#[test]
fn duplicates_far_apart_still_collapse() {
    let src = "import a.B;\nimport a.C;\nimport a.B;\n\nclass D {}\n";
    let found = fix(src);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].edits[0].new_text, "import a.B;\nimport a.C;\n");
}
