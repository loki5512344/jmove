//! Shared helpers for the CLI integration tests.
//!
//! Each test copies a fixture tree into a fresh tempdir, runs the real
//! binary with `--root <tmp>` and asserts on exit codes, stdout/stderr and
//! the resulting files on disk.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::assert::Assert;
use tempfile::TempDir;

/// Recursively copy `tests/<language>/<name>` into a tempdir and return it.
///
/// An empty `.git` marker is created in the copy: the `ignore` crate only
/// applies `.gitignore` rules inside a git repository by default, and the
/// typescript `normal` fixture relies on its `node_modules/` rule being
/// effective.
pub fn copy_fixture(language: &str, name: &str) -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    copy_dir(&fixture_dir(language, name), tmp.path());
    fs::create_dir(tmp.path().join(".git")).expect("git marker");
    tmp
}

/// Source path of a fixture tree inside the repository.
pub fn fixture_dir(language: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(language)
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
pub fn jmove(root: &TempDir, args: &[&str]) -> Assert {
    let mut cmd = Command::cargo_bin("jmove").expect("jmove binary");
    cmd.arg("--root").arg(root.path()).args(args);
    cmd.assert()
}

/// Read `path` as a string, panicking with the path on failure.
pub fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Build a path inside the tempdir root from a `/`-separated relative path.
pub fn in_root(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}
