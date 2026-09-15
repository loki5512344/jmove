//! End-to-end tests for the `java/missing-import` rule against fixtures.
//! Self-contained copy of the tiny helpers it needs (each test file is its
//! own crate; importing all of `common` would trip dead-code warnings).

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};

fn fixture(name: &str) -> TempDir {
    let tmp = tempdir().expect("tempdir");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("java")
            .join(name),
        tmp.path(),
    );
    fs::create_dir(tmp.path().join(".git")).expect("git marker");
    tmp
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    for entry in fs::read_dir(from).expect("readdir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("filetype").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

fn jmove(root: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("jmove").expect("jmove binary");
    cmd.arg("--root").arg(root.path()).args(args);
    cmd.assert()
}

fn in_root(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {path:?}: {err}"))
}

fn missing_project() -> tempfile::TempDir {
    fixture("fix_missing")
}

#[test]
fn missing_import_adds_unique_and_reports_ambiguous() {
    let tmp = missing_project();
    jmove(&tmp, &["fix", "--rule", "java/missing-import", "--json"])
        .success()
        .stdout(
            predicate::str::contains("\"rule\": \"java/missing-import\"")
                .and(predicate::str::contains("\"applied\": false"))
                .and(predicate::str::contains("\"com.example.a.Config\""))
                .and(predicate::str::contains("\"com.example.b.Config\"")),
        );
    let calc = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/Calc.java",
    ));
    assert!(
        calc.contains("package com.example.app;\nimport com.example.util.Maths;\n"),
        "{calc}"
    );
    // The ambiguous `Config` is never guessed at.
    let refer = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/c/Refer.java",
    ));
    assert!(!refer.contains("import com.example."), "{refer}");
    jmove(&tmp, &["check"]).success();
}

#[test]
fn missing_import_dry_run_then_stable_reapply() {
    let tmp = missing_project();
    jmove(&tmp, &["fix", "--dry-run"])
        .success()
        .stdout(predicate::str::contains("+import com.example.util.Maths;"));
    jmove(&tmp, &["fix"])
        .success()
        .stdout(predicate::str::contains("fixed 3 issues in 2 files"));
    // Only manual (ambiguous) findings remain: nothing further applies.
    jmove(&tmp, &["fix"])
        .success()
        .stdout(predicate::str::contains("nothing to change"));
}
