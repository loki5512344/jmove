//! `mv` warns (never edits) about non-import references: markdown links,
//! `package.json` fields and `jest.mock` strings survive silent unless the
//! user sees them. JSON payload carries the same list for agents.

mod common;

use common::{copy_fixture, in_root, jmove, read};
use predicates::prelude::*;

fn dry_run_refs() -> String {
    let tmp = copy_fixture("typescript", "refs");
    let out = jmove(
        &tmp,
        &["mv", "lib/sum.ts", "lib/total.ts", "--dry-run", "--json"],
    )
    .success()
    .stdout(predicate::str::contains("\"non_import_refs\""))
    .get_output()
    .stdout
    .clone();
    String::from_utf8(out).expect("json is utf-8")
}

#[test]
fn json_dry_run_lists_hidden_references_and_skips_the_noise() {
    let out = dry_run_refs();
    for expected in [
        "\"file\": \"README.md\"",
        "\"file\": \"package.json\"",
        "\"file\": \"__tests__/sum.test.ts\"",
    ] {
        assert!(out.contains(expected), "missing {expected} in {out}");
    }
    for absent in [
        "\"file\": \"app.ts\"",
        "\"file\": \"package-lock.json\"",
        "\"file\": \".notes/refs.md\"",
        "\"file\": \"lib/summary.ts\"",
    ] {
        assert!(!out.contains(absent), "leaked {absent} in {out}");
    }
}

#[test]
fn human_dry_run_warns_on_stderr_and_exits_zero() {
    let tmp = copy_fixture("typescript", "refs");
    jmove(&tmp, &["mv", "lib/sum.ts", "lib/total.ts", "--dry-run"])
        .success()
        .stderr(
            predicate::str::contains("3 non-import references to moved files")
                .and(predicate::str::contains("README.md:1"))
                .and(predicate::str::contains("__tests__/sum.test.ts:1")),
        );
}

#[test]
fn real_move_still_succeeds_and_still_warns() {
    let tmp = copy_fixture("typescript", "refs");
    jmove(&tmp, &["mv", "lib/sum.ts", "lib/total.ts"])
        .success()
        .stderr(predicate::str::contains("may need manual fixing"));
    assert!(in_root(tmp.path(), "lib/total.ts").exists());
    let app = read(&in_root(tmp.path(), "app.ts"));
    assert!(app.contains("from './lib/total'"), "{app}");
    // The scanner warns; only the import graph is rewritten.
    let readme = read(&in_root(tmp.path(), "README.md"));
    assert!(readme.contains("./lib/sum.ts"), "{readme}");
}
