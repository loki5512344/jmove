//! End-to-end tests for the `jmove` CLI against the Java fixtures.

mod common;

use common::{copy_fixture, in_root, jmove, read};
use predicates::prelude::*;

fn fixture(name: &str) -> tempfile::TempDir {
    copy_fixture("java", name)
}

const TEXT: &str = "src/main/java/com/example/util/Text.java";
const MOVED: &str = "src/main/java/com/example/core/Text.java";

#[test]
fn mv_between_packages_updates_declaration_and_every_importer() {
    let tmp = fixture("basic");
    jmove(&tmp, &["mv", TEXT, MOVED])
        .success()
        .stdout(predicate::str::contains("4 imports in 3 files"));

    let moved = read(&in_root(tmp.path(), MOVED));
    assert!(moved.contains("package com.example.core;"), "{moved}");
    assert!(!moved.contains("com.example.util"), "{moved}");

    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(app.contains("import com.example.core.Text;"), "{app}");
    assert!(
        app.contains("import static com.example.core.Text.shout;"),
        "{app}"
    );
    // Specifiers that never named the moved class are byte-identical.
    assert!(app.contains("import com.example.unknown.*;"), "{app}");
    assert!(app.contains("import java.util.List;"), "{app}");

    let user = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/model/User.java",
    ));
    assert!(user.contains("import com.example.core.Text;"), "{user}");
    assert!(!in_root(tmp.path(), TEXT).exists());

    // Unresolvable Java imports (jdk, wildcard) are not "broken".
    jmove(&tmp, &["check"]).success();
}

#[test]
fn mv_dry_run_previews_all_three_edits_without_writing() {
    let tmp = fixture("basic");
    jmove(&tmp, &["mv", TEXT, MOVED, "--dry-run"])
        .success()
        .stdout(predicate::str::contains("-package com.example.util;"))
        .stdout(predicate::str::contains("+package com.example.core;"))
        .stdout(predicate::str::contains("-import com.example.util.Text;"))
        .stdout(predicate::str::contains("+import com.example.core.Text;"))
        .stdout(predicate::str::contains(format!("move {TEXT} -> {MOVED}")));
    assert!(in_root(tmp.path(), TEXT).is_file());
    assert!(!in_root(tmp.path(), MOVED).exists());
}

#[test]
fn mv_json_reports_package_and_importer_changes() {
    let tmp = fixture("basic");
    jmove(&tmp, &["mv", TEXT, MOVED, "--json"])
        .success()
        .stdout(
            predicate::str::contains("\"old\": \"com.example.util\"")
                .and(predicate::str::contains("\"new\": \"com.example.core\""))
                .and(predicate::str::contains(
                    "\"old\": \"com.example.util.Text.shout\"",
                ))
                .and(predicate::str::contains(
                    "\"new\": \"com.example.core.Text.shout\"",
                ))
                .and(predicate::str::contains("\"updated_imports\": 4")),
        );
    let changed = read(&in_root(tmp.path(), MOVED));
    assert!(changed.contains("package com.example.core"));
}

#[test]
fn rename_inside_same_package_keeps_package_declaration() {
    let tmp = fixture("basic");
    jmove(
        &tmp,
        &["mv", TEXT, "src/main/java/com/example/util/Paragraph.java"],
    )
    .success();
    let renamed = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/util/Paragraph.java",
    ));
    assert!(renamed.contains("package com.example.util;"), "{renamed}");
    let app = read(&in_root(
        tmp.path(),
        "src/main/java/com/example/app/App.java",
    ));
    assert!(app.contains("import com.example.util.Paragraph;"), "{app}");
    assert!(
        app.contains("import static com.example.util.Paragraph.shout;"),
        "{app}"
    );
}

#[test]
fn target_outside_the_source_root_is_rejected() {
    let tmp = fixture("basic");
    jmove(&tmp, &["mv", TEXT, "webapp/core/Text.java", "--json"])
        .code(1)
        .stdout(
            predicate::str::contains("\"code\": \"PLAN_REJECTED\"")
                .and(predicate::str::contains("source root")),
        );
    assert!(in_root(tmp.path(), TEXT).is_file());
}

#[test]
fn java_file_must_keep_the_java_extension() {
    let tmp = fixture("basic");
    jmove(
        &tmp,
        &["mv", TEXT, "src/main/java/com/example/core/Text.txt"],
    )
    .code(1)
    .stderr(predicate::str::contains(".java extension"));
}

#[test]
fn default_package_file_cannot_change_directory() {
    let tmp = fixture("basic");
    jmove(
        &tmp,
        &[
            "mv",
            "src/main/java/Main.java",
            "src/main/java/app/Main.java",
        ],
    )
    .code(1)
    .stderr(predicate::str::contains("default package"));
    // ...but an in-place rename is fine and needs no rewrites.
    jmove(
        &tmp,
        &["mv", "src/main/java/Main.java", "src/main/java/Run.java"],
    )
    .success();
    assert!(in_root(tmp.path(), "src/main/java/Run.java").is_file());
}

fn monorepo() -> tempfile::TempDir {
    fixture("monorepo")
}

#[test]
fn source_root_scopes_the_move_to_one_duplicate_tree() {
    let tmp = monorepo();
    jmove(
        &tmp,
        &[
            "mv",
            "--source-root",
            "guava",
            "guava/src/com/example/Primitives.java",
            "guava/src/com/example/util/Primitives.java",
        ],
    )
    .success()
    .stdout(predicate::str::contains("updated 2 imports in 2 files"));

    // The scoped tree moved and its importer followed the new package.
    let app = read(&in_root(tmp.path(), "guava/src/com/example/app/App.java"));
    assert!(app.contains("import com.example.util.Primitives;"), "{app}");
    let moved = read(&in_root(
        tmp.path(),
        "guava/src/com/example/util/Primitives.java",
    ));
    assert!(moved.contains("package com.example.util;"), "{moved}");

    // The sibling copy is a self-contained tree: not one byte touched.
    let android = read(&in_root(
        tmp.path(),
        "android/guava/src/com/example/app/App.java",
    ));
    assert!(
        android.contains("import com.example.Primitives;"),
        "{android}"
    );
    assert!(in_root(tmp.path(), "android/guava/src/com/example/Primitives.java").is_file());
    jmove(&tmp, &["check", "--source-root", "guava"]).success();
}

#[test]
fn unknown_source_root_is_rejected() {
    let tmp = monorepo();
    jmove(&tmp, &["check", "--source-root", "nope"])
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "--source-root 'nope' is not a directory",
        ));
}
