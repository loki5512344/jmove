use super::{Index, ResolvedImport};
use crate::core::JmoveResult;
use crate::parser::ImportRecord;
use std::fs;
use std::path::{Path, PathBuf};

// Write `rel` (creating parent dirs) inside `root`.
fn write_file(root: &Path, rel: &str, body: &str) -> JmoveResult<()> {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, body)?;
    Ok(())
}

// Stub edge with resolved target `target` (parser-independent).
fn edge(target: &str) -> ResolvedImport {
    let record = ImportRecord {
        specifier: format!("./{target}"),
        span: 0..0,
        is_dynamic: false,
    };
    ResolvedImport {
        record,
        target: Some(PathBuf::from(target)),
    }
}

#[test]
fn build_skips_ignored_and_unsupported_files() -> JmoveResult<()> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    fs::create_dir(root.join(".git"))?;
    write_file(root, ".gitignore", "ignored/\n")?;
    write_file(root, "src/a.ts", "import { b } from './b';\n")?;
    write_file(root, "src/b.ts", "export const b = 1;\n")?;
    write_file(root, "src/legacy.js", "const a = require('./a');\n")?;
    write_file(root, "ignored/c.ts", "export const c = 1;\n")?;
    write_file(root, "docs/note.md", "not source\n")?;
    fs::write(root.join("src/binary.ts"), [0xff_u8, 0xfe, 0x00, 0x01])?;

    let index = Index::build(root)?;
    assert!(index.files.contains(Path::new("src/a.ts")));
    assert!(index.files.contains(Path::new("src/b.ts")));
    assert!(index.files.contains(Path::new("src/legacy.js")));
    // gitignored, non-source and non-UTF-8 files must never be indexed.
    for skipped in ["ignored/c.ts", "docs/note.md", "src/binary.ts"] {
        assert!(!index.files.contains(Path::new(skipped)), "{skipped}");
    }
    Ok(())
}

#[test]
fn build_resolves_relative_specifier_to_indexed_file() -> JmoveResult<()> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    write_file(root, "src/a.ts", "import { b } from './b';\n")?;
    write_file(root, "src/b.ts", "export const b = 1;\n")?;

    let index = Index::build(root)?;
    let imports = index
        .imports
        .get(Path::new("src/a.ts"))
        .expect("a.ts must be indexed");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].record.specifier, "./b");
    assert_eq!(imports[0].target.as_deref(), Some(Path::new("src/b.ts")));
    assert_eq!(index.root, fs::canonicalize(root)?);
    Ok(())
}

#[test]
fn java_imports_resolve_through_declared_packages() -> JmoveResult<()> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    write_file(
        root,
        "src/main/java/com/example/App.java",
        "package com.example;\n\nimport com.example.util.Text;\nimport java.util.List;\n\npublic class App {}\n",
    )?;
    write_file(
        root,
        "src/main/java/com/example/util/Text.java",
        "package com.example.util;\n\npublic class Text {}\n",
    )?;

    let index = Index::build(root)?;
    let decl = &index.packages[Path::new("src/main/java/com/example/util/Text.java")];
    assert_eq!(decl.name, "com.example.util");
    let imports = &index.imports[Path::new("src/main/java/com/example/App.java")];
    assert_eq!(
        imports[0].target.as_deref(),
        Some(Path::new("src/main/java/com/example/util/Text.java"))
    );
    // External (jdk) imports stay unresolved, like TS bare specifiers.
    assert_eq!(imports[1].target, None);
    Ok(())
}

#[test]
fn importers_of_returns_sorted_reverse_edges() -> JmoveResult<()> {
    let dir = tempfile::TempDir::new()?;
    let mut index = Index::build(dir.path())?;
    // Stub the graph so reverse-edge logic is independent of the parser.
    index.imports.clear();
    let edges = [
        ("z.ts", "shared.ts"),
        ("a.ts", "shared.ts"),
        ("m.ts", "other.ts"),
    ];
    for (file, target) in edges {
        index
            .imports
            .insert(PathBuf::from(file), vec![edge(target)]);
    }
    assert_eq!(
        index.importers_of(Path::new("shared.ts")),
        vec![PathBuf::from("a.ts"), PathBuf::from("z.ts")]
    );
    assert!(index.importers_of(Path::new("missing.ts")).is_empty());
    Ok(())
}

// The guava monorepo shape: `android/guava` and `guava` both declare
// `com.example.App`, so an unscoped index resolves every importer to the
// same (sorted-first) copy and a move in one tree rewrites the wrong files.
fn monorepo() -> JmoveResult<tempfile::TempDir> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    for tree in ["guava", "android/guava"] {
        write_file(
            root,
            &format!("{tree}/src/com/example/App.java"),
            "package com.example;\npublic class App {}\n",
        )?;
        write_file(
            root,
            &format!("{tree}/src/com/example/Use.java"),
            "package com.example;\nimport com.example.App;\nclass Use { App a; }\n",
        )?;
    }
    Ok(dir)
}

fn resolved_target(index: &Index, importer: &str) -> Option<PathBuf> {
    index
        .imports
        .get(Path::new(importer))?
        .iter()
        .find_map(|r| r.target.clone())
}

#[test]
fn unscoped_index_resolves_the_duplicate_fqn_to_the_sorted_first_tree() -> JmoveResult<()> {
    let dir = monorepo()?;
    let index = Index::build(dir.path())?;
    // Both trees' `App` collide; the class index keeps `android/guava` (sorts first).
    assert_eq!(
        resolved_target(&index, "guava/src/com/example/Use.java").as_deref(),
        Some(Path::new("android/guava/src/com/example/App.java")),
        "demonstrates the collision the flag exists to fix"
    );
    Ok(())
}

#[test]
fn source_root_scopes_indexing_so_the_fqn_resolves_within_the_tree() -> JmoveResult<()> {
    let dir = monorepo()?;
    let index = Index::build_scoped(dir.path(), Some(Path::new("guava")))?;
    // Only the guava tree is indexed at all.
    assert!(
        index
            .files
            .contains(Path::new("guava/src/com/example/App.java"))
    );
    assert!(
        !index
            .files
            .contains(Path::new("android/guava/src/com/example/App.java"))
    );
    assert_eq!(
        resolved_target(&index, "guava/src/com/example/Use.java").as_deref(),
        Some(Path::new("guava/src/com/example/App.java")),
        "the scoped index resolves to the same tree"
    );
    Ok(())
}

#[test]
fn source_root_does_not_leak_siblings_sharing_a_name_prefix() -> JmoveResult<()> {
    // `--source-root guava` must match the directory `guava`, not `guavaX`.
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    write_file(root, "guava/src/A.java", "package p;\npublic class A {}\n")?;
    write_file(root, "guavax/src/B.java", "package p;\npublic class B {}\n")?;
    let index = Index::build_scoped(root, Some(Path::new("guava")))?;
    assert!(index.files.contains(Path::new("guava/src/A.java")));
    assert!(!index.files.contains(Path::new("guavax/src/B.java")));
    Ok(())
}
