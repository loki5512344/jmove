//! Tests for the Java frontend and the FQN class index.

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
fn single_segment_package_is_not_a_scoped_identifier() {
    // `package p;` has no dots: the grammar yields a bare `identifier`.
    let src = "package p;\n\npublic class P {}\n";
    let decl = TreeSitterJava::new().extract_package(src).expect("package");
    assert_eq!(decl.name, "p");
    assert_eq!(&src[decl.span.clone()], "p");
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
    let mut by_simple = HashMap::new();
    by_simple.insert("Parser".to_owned(), vec!["com.example.Parser".to_owned()]);
    let index = JavaClassIndex { classes, by_simple };
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
    assert_eq!(
        index.candidates("Parser"),
        ["com.example.Parser".to_owned()].as_slice()
    );
    assert!(index.candidates("List").is_empty());
}
