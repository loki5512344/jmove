//! End-to-end tests for the `jmove` CLI against the TypeScript fixtures.

mod common;

use common::{copy_fixture, fixture_dir, in_root, jmove, read};
use predicates::prelude::*;
use tempfile::TempDir;

/// Convenience wrapper: a TypeScript fixture by name.
fn fixture(name: &str) -> TempDir {
    copy_fixture("typescript", name)
}

#[test]
fn mv_dry_run_prints_diff_and_leaves_disk_untouched() {
    let tmp = fixture("basic");
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
        read(&fixture_dir("typescript", "basic").join("app.ts")),
        "importer untouched"
    );
}

#[test]
fn mv_rewrites_importer_and_moves_the_file() {
    let tmp = fixture("basic");
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
    let tmp = fixture("normal");
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
    let tmp = fixture("complex");
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
    let clean = fixture("basic");
    jmove(&clean, &["check"])
        .success()
        .stdout(predicate::str::contains("no broken imports"));

    let messy = fixture("complex");
    jmove(&messy, &["check"])
        .code(2)
        .stdout(predicate::str::contains(
            "src/broken.ts:4: cannot resolve './gone'",
        ));
}

#[test]
fn check_json_reports_broken_import_payload() {
    let tmp = fixture("complex");
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
    let tmp = fixture("basic");
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
    let tmp = fixture("basic");
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
    let tmp = fixture("basic");
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
    let tmp = fixture("basic");
    jmove(&tmp, &["mv", "lib/nope.ts", "utils/nope.ts", "--json"])
        .code(1)
        .stdout(predicate::str::contains("\"code\": \"SOURCE_NOT_FOUND\""));
}
