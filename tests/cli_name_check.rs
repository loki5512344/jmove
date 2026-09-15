//! `jmove check` finds Java files whose public type is misnamed, and the
//! suggested `jmove mv` repairs the layout.

mod common;

use common::{copy_fixture, in_root, jmove, read};
use predicates::prelude::*;

const BAD: &str = "src/main/java/com/example/Bad.java";

#[test]
fn check_reports_misnamed_public_class_and_clean_files_stay_quiet() {
    let tmp = copy_fixture("java", "mismatch");
    jmove(&tmp, &["check"])
        .code(2)
        .stdout(
            predicate::str::contains(
                "src/main/java/com/example/Bad.java:3: public class 'Wrong' must live in 'src/main/java/com/example/Wrong.java'",
            )
            .and(predicate::str::contains("Good.java").not()),
        );
    jmove(&tmp, &["check", "--json"])
        .code(2)
        .stdout(
            predicate::str::contains("\"name_mismatches\"")
                .and(predicate::str::contains("\"public_class\": \"Wrong\""))
                .and(predicate::str::contains(
                    "\"rename\": \"jmove mv 'src/main/java/com/example/Bad.java' 'src/main/java/com/example/Wrong.java'\"",
                )),
        );
}

#[test]
fn running_the_suggested_rename_makes_check_pass() {
    let tmp = copy_fixture("java", "mismatch");
    jmove(&tmp, &["mv", BAD, "src/main/java/com/example/Wrong.java"]).success();
    let moved = read(&in_root(tmp.path(), "src/main/java/com/example/Wrong.java"));
    assert!(moved.contains("public class Wrong {}"), "{moved}");
    assert!(moved.contains("package com.example;"), "{moved}");
    jmove(&tmp, &["check"])
        .success()
        .stdout(predicate::str::contains("no findings"));
}
