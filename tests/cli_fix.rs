//! End-to-end tests for `jmove fix` against a Java fixture.

mod common;

use common::{in_root, jmove, read};
use predicates::prelude::*;

fn dirty_project() -> tempfile::TempDir {
    common::copy_fixture("java", "fix")
}

#[test]
fn fix_dry_run_previews_deletions_without_writing() {
    let tmp = dirty_project();
    jmove(&tmp, &["fix", "--dry-run"])
        .success()
        .stdout(predicate::str::contains(
            "-import com.example.unused.Ghost;",
        ))
        .stdout(predicate::str::contains(" import java.util.List;"));
    // Disk untouched: the unused import is still there.
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(app.contains("import com.example.unused.Ghost;"), "{app}");
}

#[test]
fn fix_removes_unused_and_keeps_used_and_string_mentions() {
    let tmp = dirty_project();
    jmove(&tmp, &["fix"])
        .success()
        .stdout(predicate::str::contains("fixed 1 issue in 1 file"));

    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    // Unused single-type import is gone, whole line removed.
    assert!(!app.contains("com.example.unused.Ghost"), "{app}");
    // Used import stays.
    assert!(app.contains("import com.example.Text;"), "{app}");
    // `List` only appears inside a string literal => kept (safe direction).
    assert!(app.contains("import java.util.List;"), "{app}");
    assert!(app.contains("\"java.util.List\""), "{app}");

    // Run 2: convergence — the deferred `java/import-order` fix (it
    // overlapped the deletion in run 1) now applies on the clean block.
    jmove(&tmp, &["fix"])
        .success()
        .stdout(predicate::str::contains("fixed 1 issue in 1 file"));
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(
        app.contains(
            "import static com.example.Text.shout;\n\nimport com.example.Text;\nimport java.util.List;\n"
        ),
        "{app}"
    );
    // Run 3: fixed point — nothing left to change.
    jmove(&tmp, &["fix"])
        .success()
        .stdout(predicate::str::contains("nothing to change"));
    // The project still checks clean.
    jmove(&tmp, &["check"]).success();
}

#[test]
fn fix_json_reports_candidates_and_applied_count() {
    let tmp = dirty_project();
    jmove(&tmp, &["fix", "--dry-run", "--json"])
        .success()
        .stdout(
            predicate::str::contains("\"status\": \"dry_run\"")
                .and(predicate::str::contains("\"operation\": \"fix\""))
                .and(predicate::str::contains("\"rule\": \"java/unused-import\""))
                .and(predicate::str::contains("\"would_fix\": 1")),
        );

    jmove(&tmp, &["fix", "--json"]).success().stdout(
        predicate::str::contains("\"status\": \"ok\"")
            .and(predicate::str::contains("\"fixes\": 1"))
            .and(predicate::str::contains("\"files_changed\": 1")),
    );
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(!app.contains("Ghost"), "{app}");
}

#[test]
fn fix_unknown_rule_is_rejected() {
    let tmp = dirty_project();
    jmove(&tmp, &["fix", "--rule", "does/not-exist"])
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "unknown fix rule 'does/not-exist'",
        ))
        .stderr(predicate::str::contains("java/unused-import"));
    // Nothing on disk changed.
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(app.contains("Ghost"), "{app}");
}

#[test]
fn fix_scoped_to_rule_and_reports_guava_like_sibling() {
    // `Text` is referenced only by a static member import of the same type,
    // so neither import is provably dead (mirrors the Guava smoke note).
    let tmp = dirty_project();
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(
        app.contains("import static com.example.Text.shout;"),
        "{app}"
    );
    jmove(&tmp, &["fix", "--rule", "java/unused-import"])
        .success()
        .stdout(predicate::str::contains("fixed 1 issue in 1 file"));
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(app.contains("import com.example.Text;"), "{app}");
    assert!(
        app.contains("import static com.example.Text.shout;"),
        "{app}"
    );
}

fn missing_project() -> tempfile::TempDir {
    common::copy_fixture("java", "fix_missing")
}

#[test]
fn unused_delete_and_missing_insert_coexist_in_one_file() {
    // Dual.java: the unused import is deleted while the missing `Maths`
    // import lands at the very byte of the deleted line's end — adjacent,
    // not overlapping, so one atomic plan carries both.
    let tmp = missing_project();
    let dual = "src/main/java/com/example/app/Dual.java";
    jmove(&tmp, &["fix"]).success();
    let text = read(&in_root(tmp.path(), dual));
    assert!(!text.contains("Gone"), "{text}");
    assert!(text.contains("import com.example.util.Maths;"), "{text}");
    jmove(&tmp, &["check"]).success();
}

fn ts_unused() -> tempfile::TempDir {
    common::copy_fixture("typescript", "unused")
}

#[test]
fn ts_unused_import_deletes_dead_type_import_and_keeps_the_rest() {
    let tmp = ts_unused();
    // The whole-statement delete must fire for the dead `Ghost` type import.
    jmove(&tmp, &["fix", "--rule", "ts/unused-import", "--json"])
        .success()
        .stdout(
            predicate::str::contains("\"rule\": \"ts/unused-import\"")
                .and(predicate::str::contains("\"applied\": true")),
        );
    let app = read(&in_root(tmp.path(), "src/app.ts"));
    assert!(!app.contains("Ghost"), "{app}");
    // Side-effect import, the mixed used/unused statement and the default
    // class import all stay (under-delete safety).
    assert!(app.contains("import './side-effects';"), "{app}");
    assert!(app.contains("unused"), "{app}");
    assert!(app.contains("import Logger"), "{app}");
    jmove(&tmp, &["check"]).success();
}
