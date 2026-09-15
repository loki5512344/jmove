//! `--report <FILE>` interop: SARIF 2.1.0 and Checkstyle XML for CI/IDE
//! consumers. The reports mirror the findings of `check`/`fix` without
//! changing stdout or exit codes.

mod common;

use common::{copy_fixture, in_root, jmove, read};
use predicates::prelude::*;

fn report_path(tmp: &tempfile::TempDir, name: &str) -> String {
    tmp.path()
        .join(name)
        .to_str()
        .expect("utf-8 path")
        .to_owned()
}

fn sarif(tmp: &tempfile::TempDir, name: &str) -> serde_json::Value {
    serde_json::from_str(&read(&tmp.path().join(name))).expect("valid sarif json")
}

#[test]
fn check_writes_sarif_and_keeps_the_findings_exit_code() {
    let tmp = copy_fixture("java", "mismatch");
    let report = report_path(&tmp, "out.sarif");
    jmove(&tmp, &["check", "--report", &report]).code(2);
    let doc = sarif(&tmp, "out.sarif");
    let result = &doc["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "java/class-name-mismatch");
    assert_eq!(result["level"], "error");
    assert!(
        result["message"]["text"]
            .as_str()
            .unwrap()
            .contains("jmove mv")
    );
    let location = &result["locations"][0]["physicalLocation"];
    assert_eq!(
        location["artifactLocation"]["uri"],
        "src/main/java/com/example/Bad.java"
    );
    assert_eq!(location["region"]["startLine"], 3);
    assert_eq!(result["properties"]["autoFixable"], true);
}

#[test]
fn check_writes_checkstyle_for_broken_imports() {
    let tmp = copy_fixture("typescript", "complex");
    let report = report_path(&tmp, "checkstyle.xml");
    jmove(&tmp, &["check", "--report", &report]).code(2);
    let xml = read(&in_root(tmp.path(), "checkstyle.xml"));
    assert!(xml.starts_with("<?xml version=\"1.0\""), "{xml}");
    assert!(xml.contains("<file name=\"src/broken.ts\">"), "{xml}");
    assert!(
        xml.contains(
            "<error line=\"4\" severity=\"error\" message=\"cannot resolve &#39;./gone&#39;\""
        ) || xml.contains("message=\"cannot resolve './gone'\""),
        "{xml}"
    );
    assert!(xml.contains("source=\"jmove.broken-import\""), "{xml}");
}

#[test]
fn clean_projects_still_get_valid_empty_reports() {
    let tmp = copy_fixture("typescript", "basic");
    let sarif_name = report_path(&tmp, "empty.sarif");
    let xml_name = report_path(&tmp, "empty.xml");
    jmove(&tmp, &["check", "--report", &sarif_name]).success();
    jmove(&tmp, &["check", "--report", &xml_name]).success();
    let doc = sarif(&tmp, "empty.sarif");
    assert_eq!(doc["version"], "2.1.0");
    assert!(doc["runs"][0]["results"].as_array().unwrap().is_empty());
    assert!(
        doc["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let xml = read(&in_root(tmp.path(), "empty.xml"));
    assert!(!xml.contains("<file"), "{xml}");
    assert!(xml.ends_with("</checkstyle>\n"), "{xml}");
}

#[test]
fn unknown_report_extension_fails_fast() {
    let tmp = copy_fixture("typescript", "basic");
    let report = report_path(&tmp, "out.txt");
    jmove(&tmp, &["check", "--report", &report])
        .code(1)
        .stderr(predicate::str::contains("unsupported file name"));
    assert!(!tmp.path().join("out.txt").exists());
}

#[test]
fn fix_reports_candidates_in_both_formats_without_touching_stdout() {
    let tmp = copy_fixture("typescript", "unused");
    let sarif_name = report_path(&tmp, "fix.sarif");
    let xml_name = report_path(&tmp, "fix.xml");
    jmove(&tmp, &["fix", "--dry-run", "--report", &sarif_name])
        .success()
        .stdout(predicate::str::contains("-import type { Ghost }"));
    jmove(&tmp, &["fix", "--dry-run", "--report", &xml_name]).success();
    let doc = sarif(&tmp, "fix.sarif");
    let result = &doc["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "ts/unused-import");
    assert_eq!(result["level"], "warning");
    assert_eq!(result["properties"]["autoFixable"], true);
    let xml = read(&in_root(tmp.path(), "fix.xml"));
    assert!(xml.contains("source=\"jmove.ts.unused-import\""), "{xml}");
    assert!(xml.contains("severity=\"warning\""), "{xml}");
}

#[test]
fn applied_fix_writes_report_before_stdout_summary() {
    let tmp = copy_fixture("typescript", "unused");
    let report = report_path(&tmp, "applied.sarif");
    jmove(&tmp, &["fix", "--report", &report])
        .success()
        .stdout(predicate::str::contains("fixed"));
    let doc = sarif(&tmp, "applied.sarif");
    assert!(!doc["runs"][0]["results"].as_array().unwrap().is_empty());
}
