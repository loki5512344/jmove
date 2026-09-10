//! End-to-end tests for the `jmove` CLI against the TypeScript fixtures.
//!
//! Each test copies a fixture tree into a fresh tempdir, runs the real
//! binary with `--root <tmp>` and asserts on exit codes, stdout/stderr and
//! the resulting files on disk.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::assert::Assert;
use predicates::prelude::*;
use tempfile::TempDir;

/// Recursively copy `tests/typescript/<name>` into a tempdir and return it.
///
/// An empty `.git` marker is created in the copy: the `ignore` crate only
/// applies `.gitignore` rules inside a git repository by default, and the
/// `normal` fixture relies on its `node_modules/` rule being effective.
fn copy_fixture(name: &str) -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let from = fixture_dir(name);
    copy_dir(&from, tmp.path());
    fs::create_dir(tmp.path().join(".git")).expect("git marker");
    tmp
}

/// Source path of a TypeScript fixture tree inside the repository.
fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/typescript")
        .join(name)
}

/// Recursive file/dir copy; plain `std::fs` only, no symlinks in fixtures.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

/// Run `jmove --root <dir> <args…>` and return an assertable outcome.
fn jmove(root: &TempDir, args: &[&str]) -> Assert {
    let mut cmd = Command::cargo_bin("jmove").expect("jmove binary");
    cmd.arg("--root").arg(root.path()).args(args);
    cmd.assert()
}

/// Read `path` as a string, panicking with the path on failure.
fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Build a path inside the tempdir root from a `/`-separated relative path.
fn in_root(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

#[test]
fn mv_dry_run_prints_diff_and_leaves_disk_untouched() {
    let tmp = copy_fixture("basic");
    jmove(&tmp, &["mv", "lib/sum.ts", "utils/sum.ts", "--dry-run"])
        .success()
        .stdout(predicate::str::contains("./lib/sum"))
        .stdout(predicate::str::contains("./utils/sum"));
    assert!(
        in_root(tmp.path(), "lib/sum.ts").is_file(),
        "source untouched"
    );
    assert!(
        !in_root(tmp.path(), "utils/sum.ts").exists(),
        "target not created"
    );
    assert_eq!(
        read(&in_root(tmp.path(), "app.ts")),
        read(&fixture_dir("basic").join("app.ts")),
        "importer untouched"
    );
}

#[test]
fn mv_rewrites_importer_and_moves_the_file() {
    let tmp = copy_fixture("basic");
    jmove(&tmp, &["mv", "lib/sum.ts", "utils/sum.ts"])
        .success()
        .stdout(predicate::str::contains("moved lib/sum.ts -> utils/sum.ts"))
        .stdout(predicate::str::contains("1 import"))
        .stdout(predicate::str::contains("1 file"));
    assert!(
        !in_root(tmp.path(), "lib/sum.ts").exists(),
        "source is gone"
    );
    assert!(
        in_root(tmp.path(), "utils/sum.ts").is_file(),
        "target exists"
    );
    assert!(
        read(&in_root(tmp.path(), "app.ts")).contains("./utils/sum"),
        "importer updated"
    );
}

#[test]
fn mv_updates_barrel_and_never_touches_node_modules() {
    let tmp = copy_fixture("normal");
    jmove(&tmp, &["mv", "src/impl/core.ts", "src/impl/calc.ts"]).success();
    assert!(
        read(&in_root(tmp.path(), "src/impl/index.ts")).contains("./calc"),
        "barrel re-export updated"
    );
    let app = read(&in_root(tmp.path(), "src/app.ts"));
    assert!(!app.contains("./impl/core"), "side-effect import updated");
    assert!(app.contains("./utils/logger"), "unrelated import preserved");
    let vendored = read(&in_root(tmp.path(), "node_modules/legacy-lib/index.ts"));
    assert!(
        vendored.contains("../../src/impl/core"),
        "ignored directories must never be rewritten"
    );
    jmove(&tmp, &["check"]).success();
}

#[test]
fn mv_rewrites_only_the_specifier_line_in_multi_line_imports() {
    let tmp = copy_fixture("complex");
    let view = in_root(tmp.path(), "src/ui/deep/nested/view.ts");
    let before: Vec<String> = read(&view).lines().map(str::to_owned).collect();

    jmove(&tmp, &["mv", "src/util/text.ts", "src/core/text.ts"]).success();

    // Second importer at another depth: the messy `./util/../util/text`
    // specifier resolves to the moved file and must be rewritten as well.
    let config = read(&in_root(tmp.path(), "src/config.ts"));
    assert!(
        config.contains("./core/text") && !config.contains("../util"),
        "messy relative specifier updated: {config}"
    );
    let after: Vec<String> = read(&view).lines().map(str::to_owned).collect();
    assert_eq!(before.len(), after.len(), "import layout preserved");
    assert_eq!(before[0], after[0], "opening line is byte-identical");
    assert_eq!(before[1], after[1], "binding line is byte-identical");
    assert_eq!(before[2], "} from \"../../../util/text\";");
    assert_eq!(
        after[2], "} from \"../../../core/text\";",
        "only the specifier line changed"
    );
}

#[test]
fn check_passes_on_clean_and_fails_on_broken_fixture() {
    let clean = copy_fixture("basic");
    jmove(&clean, &["check"])
        .success()
        .stdout(predicate::str::contains("no broken imports"));

    let messy = copy_fixture("complex");
    jmove(&messy, &["check"])
        .code(2)
        .stdout(predicate::str::contains(
            "src/broken.ts:4: cannot resolve './gone'",
        ));
}

#[test]
fn check_json_reports_broken_import_payload() {
    let tmp = copy_fixture("complex");
    jmove(&tmp, &["check", "--json"]).code(2).stdout(
        predicate::str::contains("\"status\": \"ok\"")
            .and(predicate::str::contains("\"operation\": \"check\""))
            .and(predicate::str::contains("\"file\": \"src/broken.ts\""))
            .and(predicate::str::contains("\"line\": 4"))
            .and(predicate::str::contains("\"import\": \"./gone\""))
            .and(predicate::str::contains("\"reason\": \"file_not_found\""))
            .and(predicate::str::contains("\"total\": 1")),
    );
}

#[test]
fn mv_json_happy_path_reports_changed_files() {
    let tmp = copy_fixture("basic");
    jmove(&tmp, &["mv", "lib/sum.ts", "utils/sum.ts", "--json"])
        .success()
        .stdout(
            predicate::str::contains("\"status\": \"ok\"")
                .and(predicate::str::contains("\"operation\": \"mv\""))
                .and(predicate::str::contains("\"source\": \"lib/sum.ts\""))
                .and(predicate::str::contains("\"target\": \"utils/sum.ts\""))
                .and(predicate::str::contains("\"path\": \"app.ts\""))
                .and(predicate::str::contains("\"line\": 1"))
                .and(predicate::str::contains("\"old\": \"./lib/sum\""))
                .and(predicate::str::contains("\"new\": \"./utils/sum\""))
                .and(predicate::str::contains("\"moved\": 1"))
                .and(predicate::str::contains("\"updated_imports\": 1")),
        );
    assert!(in_root(tmp.path(), "utils/sum.ts").is_file());
}

#[test]
fn mv_json_dry_run_reports_preview_payload() {
    let tmp = copy_fixture("basic");
    jmove(
        &tmp,
        &["mv", "lib/sum.ts", "utils/sum.ts", "--dry-run", "--json"],
    )
    .success()
    .stdout(
        predicate::str::contains("\"status\": \"dry_run\"")
            .and(predicate::str::contains("\"would_move\": \"lib/sum.ts\""))
            .and(predicate::str::contains("\"would_update\": 1"))
            .and(predicate::str::contains("\"affected_files\""))
            .and(predicate::str::contains("\"app.ts\"")),
    );
    assert!(
        in_root(tmp.path(), "lib/sum.ts").is_file(),
        "dry-run writes nothing"
    );
}

#[test]
fn mv_json_reports_target_exists_error_shape() {
    let tmp = copy_fixture("basic");
    jmove(&tmp, &["mv", "app.ts", "lib/sum.ts", "--json"])
        .code(1)
        .stdout(
            predicate::str::contains("\"status\": \"error\"")
                .and(predicate::str::contains("\"operation\": \"mv\""))
                .and(predicate::str::contains("\"code\": \"TARGET_EXISTS\""))
                .and(predicate::str::contains("\"hint\"")),
        );
    assert!(
        read(&in_root(tmp.path(), "lib/sum.ts")).contains("export function sum"),
        "rejected move changes nothing"
    );
}

#[test]
fn mv_reports_source_not_found() {
    let tmp = copy_fixture("basic");
    jmove(&tmp, &["mv", "lib/nope.ts", "utils/nope.ts", "--json"])
        .code(1)
        .stdout(predicate::str::contains("\"code\": \"SOURCE_NOT_FOUND\""));
}
