//! End-to-end tests for moving whole directories: every indexed member
//! relocates in one atomic batch and all importers follow.

mod common;

use common::{copy_fixture, in_root, jmove, read};
use predicates::prelude::*;
use tempfile::TempDir;

fn fixture(name: &str) -> TempDir {
    copy_fixture("typescript", name)
}

#[test]
fn ts_dir_move_relocates_members_and_rewrites_importers() {
    let tmp = fixture("normal");
    jmove(&tmp, &["mv", "src/impl", "src/impl2"])
        .success()
        .stdout(predicate::str::contains("(2 files)"));
    assert!(!in_root(tmp.path(), "src/impl").exists());
    assert!(in_root(tmp.path(), "src/impl2/core.ts").is_file());
    assert!(in_root(tmp.path(), "src/impl2/index.ts").is_file());

    let app = read(&in_root(tmp.path(), "src/app.ts"));
    assert!(app.contains("./impl2/core"), "{app}");
    // A barrel directory import keeps its conservative `.../index` form.
    let root = read(&in_root(tmp.path(), "src/index.ts"));
    assert!(root.contains("export * from \"./impl2/index\""), "{root}");
    jmove(&tmp, &["check"]).success();
}

#[test]
fn java_package_dir_move_rewrites_packages_and_imports() {
    let tmp = copy_fixture("java", "basic");
    jmove(
        &tmp,
        &[
            "mv",
            "src/main/java/com/example/util",
            "src/main/java/com/example/core",
        ],
    )
    .success()
    .stdout(predicate::str::contains("updated 4 imports in 3 files"));

    let text = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/core/Text.java",
    ));
    assert!(text.contains("package com.example.core;"), "{text}");
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(app.contains("import com.example.core.Text;"), "{app}");
    assert!(
        app.contains("import static com.example.core.Text.shout;"),
        "{app}"
    );
    jmove(&tmp, &["check"]).success();
}

#[test]
fn dir_move_json_lists_every_file_and_unsupported_left_behind() {
    let tmp = fixture("normal");
    let note = in_root(tmp.path(), "src/impl/notes.txt");
    std::fs::write(&note, "asset, not source\n").expect("write note");
    jmove(&tmp, &["mv", "src/impl", "src/impl2", "--json"])
        .success()
        .stdout(
            predicate::str::contains("\"moved\": 2")
                .and(predicate::str::contains("\"moved_files\""))
                .and(predicate::str::contains("\"to\": \"src/impl2/core.ts\""))
                .and(predicate::str::contains("left_behind"))
                .and(predicate::str::contains("src/impl/notes.txt")),
        );
    // The unindexable asset is reported, not silently dragged along.
    assert!(note.is_file());
    assert!(!in_root(tmp.path(), "src/impl2/notes.txt").exists());
}

#[test]
fn dry_run_dir_move_previews_every_relocation() {
    let tmp = fixture("normal");
    jmove(
        &tmp,
        &["mv", "src/impl", "src/impl2", "--dry-run", "--json"],
    )
    .success()
    .stdout(
        predicate::str::contains("\"would_move_files\"")
            .and(predicate::str::contains("\"from\": \"src/impl/core.ts\"")),
    );
    assert!(
        in_root(tmp.path(), "src/impl/core.ts").is_file(),
        "dry-run writes nothing"
    );
}
