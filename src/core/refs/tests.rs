//! Scanner tests: what may be referenced, what must never match.

use std::path::Path;

use crate::core::JmoveResult;
use crate::core::index::Index;
use crate::core::plan::plan_move;
use tempfile::TempDir;

use super::{NonImportRef, scan, token_occurrences};

fn fixture() -> JmoveResult<TempDir> {
    let dir = TempDir::new()?;
    let root = dir.path();
    let write = |rel: &str, body: &str| -> JmoveResult<()> {
        fs_create(rel, body, root)?;
        Ok(())
    };
    write("lib/sum.ts", "export const sum = 3;\n")?;
    write("lib/summary.ts", "export const summary = 's';\n")?;
    write("app.ts", "import { sum } from './lib/sum';\n")?;
    write(
        "README.md",
        "Use [sum](./lib/sum.ts) via `lib/sum`.\nSee also lib/summary for text.\nLayout note: lib/ holds helpers.\n",
    )?;
    write(
        "package.json",
        "{\"name\": \"refs\", \"main\": \"./lib/sum.ts\"}\n",
    )?;
    write("__tests__/sum.test.ts", "jest.mock('../lib/sum');\n")?;
    write("package-lock.json", "{\"x\": \"./lib/sum\"}\n")?;
    write(".notes/refs.md", "stale: ./lib/sum\n")?;
    Ok(dir)
}

fn fs_create(rel: &str, body: &str, root: &Path) -> std::io::Result<()> {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, body)
}

fn files_of(refs: &[NonImportRef]) -> Vec<String> {
    refs.iter().map(|r| r.file.clone()).collect()
}

fn scan_move(src: &str, dst: &str) -> JmoveResult<(TempDir, Vec<NonImportRef>)> {
    let dir = fixture()?;
    let index = Index::build(dir.path())?;
    let plan = plan_move(&index, Path::new(src), Path::new(dst))?;
    let refs = scan(dir.path(), &index, &plan);
    Ok((dir, refs))
}

#[test]
fn doc_config_and_mock_references_are_reported() -> JmoveResult<()> {
    let (_dir, refs) = scan_move("lib/sum.ts", "lib/total.ts")?;
    let files = files_of(&refs);
    for expected in ["README.md", "package.json", "__tests__/sum.test.ts"] {
        assert!(
            files.contains(&expected.to_string()),
            "{expected} missing: {refs:?}"
        );
    }
    Ok(())
}

#[test]
fn the_import_statement_itself_is_never_reported() -> JmoveResult<()> {
    let (_dir, refs) = scan_move("lib/sum.ts", "lib/total.ts")?;
    assert!(!files_of(&refs).contains(&"app.ts".to_string()), "{refs:?}");
    assert!(!files_of(&refs).contains(&"lib/sum.ts".to_string()));
    Ok(())
}

#[test]
fn decoys_lockfiles_and_hidden_dirs_are_not_reported() -> JmoveResult<()> {
    let (_dir, refs) = scan_move("lib/sum.ts", "lib/total.ts")?;
    let files = files_of(&refs);
    for absent in ["lib/summary.ts", "package-lock.json", ".notes/refs.md"] {
        assert!(
            !files.contains(&absent.to_string()),
            "{absent} leaked: {refs:?}"
        );
    }
    // README line 2 mentions only `lib/summary`: no ref may point there.
    assert!(
        refs.iter().all(|r| r.file != "README.md" || r.line != 2),
        "{refs:?}"
    );
    Ok(())
}

#[test]
fn one_entry_per_line_keeps_the_highest_ranked_token() -> JmoveResult<()> {
    let (_dir, refs) = scan_move("lib/sum.ts", "lib/total.ts")?;
    let readme = refs.iter().filter(|r| r.file == "README.md").count();
    // Line 1 (three matching tokens) and line 3 (`lib/` dir note is only a
    // file-move token set away) collapse to one entry per line.
    assert_eq!(readme, 1, "{refs:?}");
    assert_eq!(refs.iter().find(|r| r.file == "README.md").unwrap().line, 1);
    Ok(())
}

#[test]
fn dir_move_reports_the_directory_prefix() -> JmoveResult<()> {
    let (_dir, refs) = scan_move("lib", "pkg/lib")?;
    assert!(
        refs.iter()
            .any(|r| r.file == "README.md" && r.line == 3 && r.kind == "dir"),
        "{refs:?}"
    );
    Ok(())
}

#[test]
fn boundary_rules_reject_longer_joined_and_prefixed_names() {
    let none: Vec<usize> = Vec::new();
    assert_eq!(token_occurrences("lib/summary", "lib/sum"), none);
    assert_eq!(token_occurrences("lib/sum-2", "lib/sum"), none);
    assert_eq!(token_occurrences("mylib/sum", "lib/sum"), none);
    assert_eq!(token_occurrences("./lib/sum.ts", "lib/sum"), vec![2]);
    assert_eq!(token_occurrences("x/lib/sum", "lib/sum"), vec![2]);
}
