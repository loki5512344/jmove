//! Unit tests for the Java move planner.

use super::{java_rewrites, rewrite_fqn};
use crate::core::index::Index;
use crate::core::plan::tests_support::edge;
use crate::parser::PackageDecl;
use std::ops::Range;
use std::path::{Path, PathBuf};

const UTILS: &str = "src/main/java/com/example/utils";
const CORE: &str = "src/main/java/com/example/core";
// Byte span of a package name in `package com.example.utils;`
const PKG_SPAN: Range<usize> = 8..25;

// Index: Parser.java in com.example.utils, imported by App.java (exact)
// and by Service.java (static member import).
fn index() -> Index {
    let mut ix = Index::default();
    for f in [
        "src/main/java/App.java",
        &format!("{UTILS}/Parser.java"),
        &format!("{UTILS}/Text.java"),
        &format!("{CORE}/Service.java"),
    ] {
        ix.files.add(PathBuf::from(f));
    }
    for (f, pkg) in [
        (format!("{UTILS}/Parser.java"), "com.example.utils"),
        (format!("{UTILS}/Text.java"), "com.example.utils"),
        (format!("{CORE}/Service.java"), "com.example.core"),
    ] {
        ix.packages.insert(
            PathBuf::from(f),
            PackageDecl {
                name: pkg.into(),
                span: PKG_SPAN,
            },
        );
    }
    ix.imports.insert(
        PathBuf::from("src/main/java/App.java"),
        vec![edge("com.example.utils.Parser", 9..30, &parser_str())],
    );
    ix.imports.insert(
        PathBuf::from(format!("{CORE}/Service.java")),
        vec![edge(
            "com.example.utils.Parser.parse",
            23..51,
            &parser_str(),
        )],
    );
    ix
}

fn parser_str() -> String {
    format!("{UTILS}/Parser.java")
}

fn parser() -> PathBuf {
    PathBuf::from(parser_str())
}

#[test]
fn move_between_packages_rewrites_package_and_all_importers() {
    let target_str = format!("{CORE}/Parser.java");
    let target = Path::new(&target_str);
    let rewrites = java_rewrites(&index(), &parser(), target).unwrap();
    assert_eq!(
        rewrites
            .iter()
            .map(|r| (
                crate::core::rel_str(&r.file),
                r.old_text.clone(),
                r.new_text.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "src/main/java/App.java".into(),
                "com.example.utils.Parser".into(),
                "com.example.core.Parser".into()
            ),
            (
                format!("{CORE}/Service.java"),
                "com.example.utils.Parser.parse".into(),
                "com.example.core.Parser.parse".into()
            ),
            (
                format!("{UTILS}/Parser.java"),
                "com.example.utils".into(),
                "com.example.core".into()
            ),
        ]
    );
    // The package rewrite carries the declaration's own span.
    assert_eq!(rewrites[2].span, PKG_SPAN);
}

#[test]
fn rename_inside_same_package_keeps_package_and_imports_fqn() {
    let target_str = format!("{UTILS}/Reader.java");
    let target = Path::new(&target_str);
    let rewrites = java_rewrites(&index(), &parser(), target).unwrap();
    let texts: Vec<(&str, &str)> = rewrites
        .iter()
        .map(|r| (r.old_text.as_str(), r.new_text.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            ("com.example.utils.Parser", "com.example.utils.Reader"),
            (
                "com.example.utils.Parser.parse",
                "com.example.utils.Reader.parse"
            ),
        ]
    );
}

#[test]
fn target_outside_source_root_is_rejected() {
    let err = java_rewrites(&index(), &parser(), Path::new("other/Parser.java")).unwrap_err();
    assert!(
        matches!(err, crate::core::JmoveError::PlanRejected(_)),
        "{err}"
    );
    assert!(err.to_string().contains("source root"), "{err}");
}

#[test]
fn target_without_java_extension_is_rejected() {
    let err = java_rewrites(
        &index(),
        &parser(),
        Path::new("src/main/java/com/example/core/Parser.class"),
    )
    .unwrap_err();
    assert!(err.to_string().contains(".java extension"), "{err}");
}

#[test]
fn package_dir_mismatch_is_rejected() {
    let mut ix = index();
    ix.packages.insert(
        parser(),
        PackageDecl {
            name: "com.wrong.pkg".into(),
            span: PKG_SPAN,
        },
    );
    let target_str = format!("{CORE}/Parser.java");
    let err = java_rewrites(&ix, &parser(), Path::new(&target_str)).unwrap_err();
    assert!(
        err.to_string().contains("does not match directory"),
        "{err}"
    );
}

#[test]
fn default_package_allows_only_in_place_rename() {
    let mut ix = index();
    ix.packages.remove(&parser());
    let same_dir_str = format!("{UTILS}/Reader.java");
    let same_dir = Path::new(&same_dir_str);
    assert!(java_rewrites(&ix, &parser(), same_dir).unwrap().is_empty());
    let other_str = format!("{CORE}/Parser.java");
    let other = Path::new(&other_str);
    assert!(
        java_rewrites(&ix, &parser(), other)
            .unwrap_err()
            .to_string()
            .contains("default package")
    );
}

#[test]
fn fqn_prefix_swap_keeps_member_suffix() {
    assert_eq!(rewrite_fqn("a.b.C", "x.y.C", "a.b.C"), "x.y.C".to_owned());
    assert_eq!(
        rewrite_fqn("a.b.C", "x.y.C", "a.b.C.method"),
        "x.y.C.method".to_owned()
    );
}
