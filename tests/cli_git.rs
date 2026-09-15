//! End-to-end tests for the git integration of `mv`: tracked files move
//! through `git mv` (staged rename, history kept), `--no-git` opts out.
//! Self-contained: it builds a real git repo from the `typescript/basic`
//! fixture instead of reusing the fake `.git` marker of `copy_fixture`.

use std::fs;
use std::path::Path;
use std::process::Command as Git;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::{TempDir, tempdir};

/// Copy the fixture into a tempdir, `git init` it and commit everything.
fn git_fixture(name: &str) -> TempDir {
    let tmp = tempdir().expect("tempdir");
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("typescript")
        .join(name);
    copy_dir(&src, tmp.path());
    git(tmp.path(), &["init", "-q", "-b", "main", "."]);
    git(tmp.path(), &["config", "user.name", "jmove-test"]);
    git(tmp.path(), &["config", "user.email", "test@test"]);
    git(tmp.path(), &["add", "-A"]);
    git(tmp.path(), &["commit", "-qm", "init"]);
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

fn git(root: &Path, args: &[&str]) {
    let status = Git::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "`git {args:?}` failed");
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let out = Git::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "`git {args:?}` failed");
    String::from_utf8(out.stdout).expect("utf-8")
}

#[test]
fn mv_in_a_git_repo_moves_via_git_mv_and_stages_it() {
    let tmp = git_fixture("basic");
    jmove(&tmp, &["mv", "lib/sum.ts", "utils/sum.ts", "--json"])
        .success()
        .stdout(predicate::str::contains("\"moved_via\": \"git\""));
    let staged = git_output(tmp.path(), &["diff", "--cached", "-M", "--name-status"]);
    assert!(
        staged.contains("lib/sum.ts") && staged.contains("utils/sum.ts"),
        "rename must be staged: {staged}"
    );
    jmove(&tmp, &["check"]).success();
}

#[test]
fn mv_dry_run_reports_would_use_git() {
    let tmp = git_fixture("basic");
    jmove(
        &tmp,
        &["mv", "lib/sum.ts", "utils/sum.ts", "--dry-run", "--json"],
    )
    .success()
    .stdout(predicate::str::contains("\"would_move_via\": \"git\""));
    assert!(
        tmp.path().join("lib/sum.ts").is_file(),
        "dry-run writes nothing"
    );
}

#[test]
fn mv_no_git_keeps_the_plain_rename_unstaged() {
    let tmp = git_fixture("basic");
    jmove(
        &tmp,
        &["mv", "lib/sum.ts", "utils/sum.ts", "--no-git", "--json"],
    )
    .success()
    .stdout(predicate::str::contains("\"moved_via\": \"fs\""));
    let staged = git_output(tmp.path(), &["diff", "--cached", "--name-only"]);
    assert!(
        staged.trim().is_empty(),
        "--no-git must not stage: {staged}"
    );
    let target = git_output(tmp.path(), &["status", "--porcelain", "--", "utils/sum.ts"]);
    assert!(target.starts_with("??"), "{target}");
}
